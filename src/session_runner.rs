//! Capture and naming are separate workers. A saved frame never waits for OCR.
use crate::{
    capture::CaptureTarget,
    naming,
    session::{CaptureHandle, ChangeConfig, ChangeDetector, SavedCapture, SessionStore, Trigger},
};
use image::RgbaImage;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};

const RUNNING: u64 = 0;
const PAUSED: u64 = 1;
const STOPPED: u64 = 2;
const PHASE_MASK: u64 = 3;
const ARM_DELAY: Duration = Duration::from_millis(700);
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Phase and generation share one atomic so a rapid pause/resume cannot make
/// work from an earlier running period appear current again.
struct RunState(AtomicU64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct StateSnapshot(u64);

impl StateSnapshot {
    fn phase(self) -> u64 {
        self.0 & PHASE_MASK
    }

    fn generation(self) -> u64 {
        self.0 >> 2
    }

    fn next(self, phase: u64) -> Self {
        Self((self.0 & !PHASE_MASK).wrapping_add(4) | phase)
    }
}

impl RunState {
    fn new() -> Self {
        Self(AtomicU64::new(RUNNING))
    }

    fn snapshot(&self) -> StateSnapshot {
        StateSnapshot(self.0.load(Ordering::SeqCst))
    }

    fn transition(&self, from: u64, to: u64) -> bool {
        let mut current = self.snapshot();
        while current.phase() == from {
            match self.0.compare_exchange(
                current.0,
                current.next(to).0,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return true,
                Err(actual) => current = StateSnapshot(actual),
            }
        }
        false
    }

    fn is_current_running(&self, expected: StateSnapshot) -> bool {
        expected.phase() == RUNNING && self.snapshot() == expected
    }

    /// An error from an old capture must not pause a resumed or stopped session.
    fn pause_if_current(&self, expected: StateSnapshot) -> bool {
        expected.phase() == RUNNING
            && self
                .0
                .compare_exchange(
                    expected.0,
                    expected.next(PAUSED).0,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                )
                .is_ok()
    }

    fn stop(&self) {
        let mut current = self.snapshot();
        while current.phase() != STOPPED {
            match self.0.compare_exchange(
                current.0,
                current.next(STOPPED).0,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return,
                Err(actual) => current = StateSnapshot(actual),
            }
        }
    }
}

struct CaptureClock {
    generation: u64,
    armed_at: Duration,
    next_sample: Duration,
    pending_manual: bool,
}

impl CaptureClock {
    fn new(generation: u64, now: Duration) -> Self {
        Self {
            generation,
            armed_at: now + ARM_DELAY,
            next_sample: now + ARM_DELAY,
            pending_manual: false,
        }
    }

    fn synchronize(&mut self, generation: u64, now: Duration) -> bool {
        if self.generation == generation {
            return false;
        }
        *self = Self::new(generation, now);
        true
    }

    fn request_manual(&mut self, generation: u64) {
        if self.generation == generation {
            self.pending_manual = true;
        }
    }

    fn ready(&self, schedule: CaptureSchedule, now: Duration) -> bool {
        now >= self.armed_at
            && (self.pending_manual
                || (schedule != CaptureSchedule::Manual && now >= self.next_sample))
    }

    fn sampled(&mut self, schedule: CaptureSchedule, interval: Duration, now: Duration) {
        self.next_sample = now
            + match schedule {
                CaptureSchedule::Interval => interval.max(Duration::from_secs(2)),
                _ => Duration::from_secs(1),
            };
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureSchedule {
    Manual,
    Interval,
    Change,
}

pub struct RunConfig {
    pub target: CaptureTarget,
    pub schedule: CaptureSchedule,
    pub interval: Duration,
    pub change: ChangeConfig,
    pub auto_name: bool,
    pub max_captures: u64,
    pub max_duration: Duration,
}

/// The worker owns scheduling and durable writes; capture and recognition are
/// replaceable boundaries so lifecycle tests run without accessing the desktop.
pub trait CaptureAdapter: Send + Sync {
    fn capture(
        &self,
        target: &CaptureTarget,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<RgbaImage, String>;
}

pub trait NamingAdapter: Send + Sync {
    fn suggest(&self, image: &RgbaImage) -> Result<Option<String>, String>;
}

struct NativeCapture;
impl CaptureAdapter for NativeCapture {
    fn capture(
        &self,
        target: &CaptureTarget,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<RgbaImage, String> {
        target.capture_cancellable(cancelled)
    }
}

struct NativeNaming;
impl NamingAdapter for NativeNaming {
    fn suggest(&self, image: &RgbaImage) -> Result<Option<String>, String> {
        naming::suggest(image).map(|suggestion| suggestion.map(|value| value.title))
    }
}

pub struct WorkerAdapters {
    pub capture: Arc<dyn CaptureAdapter>,
    pub naming: Arc<dyn NamingAdapter>,
}

impl Default for WorkerAdapters {
    fn default() -> Self {
        Self {
            capture: Arc::new(NativeCapture),
            naming: Arc::new(NativeNaming),
        }
    }
}

pub enum SessionEvent {
    Saved(CaptureHandle, RgbaImage),
    Named {
        old_path: PathBuf,
        capture: SavedCapture,
    },
    Note(String),
    Paused(String),
    Finished(String),
}

pub struct SessionRunner {
    state: Arc<RunState>,
    drained: Arc<AtomicBool>,
    commands: mpsc::SyncSender<u64>,
    pub events: mpsc::Receiver<SessionEvent>,
    pub folder: PathBuf,
}

impl SessionRunner {
    pub fn start(
        store: SessionStore,
        config: RunConfig,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        Self::start_with_adapters(store, config, WorkerAdapters::default(), wake)
    }

    pub fn start_with_adapters(
        store: SessionStore,
        config: RunConfig,
        adapters: WorkerAdapters,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let state = Arc::new(RunState::new());
        let (commands, rx) = mpsc::sync_channel(8);
        let (tx, events) = mpsc::channel();
        let folder = store.folder().to_path_buf();
        let worker_state = state.clone();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
        let drained = Arc::new(AtomicBool::new(false));
        let worker_drained = drained.clone();
        std::thread::spawn(move || {
            let finish = std::panic::catch_unwind(std::panic::AssertUnwindSafe(||run(store,config,adapters,worker_state.clone(),rx,tx.clone(),wake.clone())))
                .unwrap_or_else(|_|"Capture worker stopped unexpectedly. Saved PNGs remain in the session folder; reopen it to review recovery.".into());
            worker_state.stop();
            worker_drained.store(true, Ordering::Release);
            send(&tx, &wake, SessionEvent::Finished(finish));
        });
        Self {
            state,
            drained,
            commands,
            events,
            folder,
        }
    }
    pub fn is_running(&self) -> bool {
        self.state.snapshot().phase() == RUNNING
    }
    pub fn is_paused(&self) -> bool {
        self.state.snapshot().phase() == PAUSED
    }
    pub fn is_stopped(&self) -> bool {
        self.state.snapshot().phase() == STOPPED
    }
    /// True only after capture persistence and queued naming metadata have finished.
    /// Stopped prevents further captures; it is not a durable-completion acknowledgement.
    pub fn is_drained(&self) -> bool {
        self.drained.load(Ordering::Acquire)
    }
    pub fn pause(&self) {
        self.state.transition(RUNNING, PAUSED);
    }
    pub fn resume(&self) {
        self.state.transition(PAUSED, RUNNING);
    }
    pub fn stop(&self) {
        self.state.stop();
    }
    pub fn capture_now(&self) -> Result<(), String> {
        let current = self.state.snapshot();
        if current.phase() != RUNNING {
            return Err("Resume the session before capturing.".into());
        }
        self.commands
            .try_send(current.generation())
            .map_err(|_| "Capture queue is busy. Give it a moment.".into())
    }
}
impl Drop for SessionRunner {
    fn drop(&mut self) {
        self.stop();
    }
}

fn send(tx: &mpsc::Sender<SessionEvent>, wake: &Arc<dyn Fn() + Send + Sync>, event: SessionEvent) {
    let _ = tx.send(event);
    wake();
}

fn run(
    mut store: SessionStore,
    config: RunConfig,
    adapters: WorkerAdapters,
    state: Arc<RunState>,
    commands: mpsc::Receiver<u64>,
    events: mpsc::Sender<SessionEvent>,
    wake: Arc<dyn Fn() + Send + Sync>,
) -> String {
    std::thread::scope(|scope| {
        let (names, name_rx) = mpsc::sync_channel::<(CaptureHandle, SavedCapture, RgbaImage)>(4);
        let name_events = events.clone();
        let name_wake = wake.clone();
        let naming_enabled = config.auto_name;
        let naming_worker = scope.spawn(move || {
            for (handle, original, pixels) in name_rx {
                match adapters.naming.suggest(&pixels) {
                    Ok(Some(title)) => match handle.rename_if_unchanged(&original, &title) {
                        Ok(Some(renamed)) => send(
                            &name_events,
                            &name_wake,
                            SessionEvent::Named {
                                old_path: original.path,
                                capture: renamed,
                            },
                        ),
                        Ok(None) => {}
                        Err(e) => send(
                            &name_events,
                            &name_wake,
                            SessionEvent::Note(format!(
                                "Image saved; automatic naming failed: {e}"
                            )),
                        ),
                    },
                    Ok(None) => {}
                    Err(e) => send(
                        &name_events,
                        &name_wake,
                        SessionEvent::Note(format!("Image saved with its fallback name. OCR: {e}")),
                    ),
                }
            }
        });
        let started = Instant::now();
        let mut detector = ChangeDetector::new(config.change.clone());
        let mut clock = CaptureClock::new(state.snapshot().generation(), started.elapsed());
        let mut count = 0;
        let finish_reason = loop {
            let current = state.snapshot();
            if current.phase() == STOPPED {
                break "Session stopped. Saved images stay in your folder.";
            }
            if started.elapsed() >= config.max_duration || count >= config.max_captures {
                break "Session limit reached. All captured images are saved.";
            }
            if clock.synchronize(current.generation(), started.elapsed()) {
                // Paused time never counts toward settling. The first stable frame
                // after resuming may repeat the last saved frame.
                detector = ChangeDetector::new(config.change.clone());
            }
            if current.phase() == PAUSED {
                std::thread::sleep(POLL_INTERVAL);
                continue;
            }
            let request = if clock.pending_manual {
                // Keep a request during arming without blocking pause/stop checks.
                std::thread::sleep(POLL_INTERVAL);
                None
            } else {
                commands.recv_timeout(POLL_INTERVAL).ok()
            };
            let current = state.snapshot();
            if clock.synchronize(current.generation(), started.elapsed()) {
                detector = ChangeDetector::new(config.change.clone());
            }
            if current.phase() != RUNNING {
                continue;
            }
            if let Some(generation) = request {
                // Old hotkeys are discarded even if pause/resume happened between
                // two worker polls. A new-generation request survives rearming.
                clock.request_manual(generation);
            }
            if !clock.ready(config.schedule, started.elapsed()) {
                continue;
            }
            if !state.is_current_running(current) {
                continue;
            }
            let manual = std::mem::take(&mut clock.pending_manual);
            let pixels = match adapters
                .capture
                .capture(&config.target, &|| !state.is_current_running(current))
            {
                Ok(image) => image,
                Err(e) => {
                    if state.pause_if_current(current) {
                        send(&events, &wake, SessionEvent::Paused(e));
                    }
                    continue;
                }
            };
            if !state.is_current_running(current) {
                continue;
            }
            let elapsed = started.elapsed();
            let trigger = if manual {
                Some(Trigger::Manual)
            } else {
                match config.schedule {
                    CaptureSchedule::Manual => None,
                    CaptureSchedule::Interval => Some(Trigger::Interval),
                    CaptureSchedule::Change => detector
                        .observe(&pixels, elapsed)
                        .then_some(Trigger::Change),
                }
            };
            if let Some(trigger) = trigger {
                // Discard pixels captured before any intervening pause/resume.
                // Once persistence starts, let the atomic file write finish.
                if !state.is_current_running(current) {
                    continue;
                }
                match store.save(&pixels, None, trigger) {
                    Ok(saved) => {
                        // Publish one shared identity before OCR or the UI can
                        // open/rename this capture. Never operate on stale paths.
                        let handle = store.handle(saved.clone());
                        count += 1;
                        detector.mark_saved(&pixels, elapsed);
                        let thumb = image::DynamicImage::ImageRgba8(pixels.clone())
                            .thumbnail(180, 110)
                            .to_rgba8();
                        send(&events, &wake, SessionEvent::Saved(handle.clone(), thumb));
                        if naming_enabled {
                            // Bounded background work. A slow recogniser cannot grow memory forever.
                            let naming_image = image::DynamicImage::ImageRgba8(pixels)
                                .thumbnail(2048, 2048)
                                .to_rgba8();
                            if names.try_send((handle, saved, naming_image)).is_err() {
                                send(
                                    &events,
                                    &wake,
                                    SessionEvent::Note(
                                        "Image saved with its fallback name; naming queue is busy."
                                            .into(),
                                    ),
                                );
                            }
                        }
                    }
                    Err(e) => {
                        if state.pause_if_current(current) {
                            send(
                                &events,
                                &wake,
                                SessionEvent::Paused(format!("Saving failed. Session paused: {e}")),
                            );
                        }
                    }
                }
            }
            clock.sampled(config.schedule, config.interval, started.elapsed());
        };
        state.stop();
        drop(names);
        if naming_worker.join().is_err() {
            return format!(
                "{finish_reason} Automatic naming stopped unexpectedly; PNGs remain saved."
            );
        }
        finish_reason.into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixtureCapture<F>(F);
    impl<F> CaptureAdapter for FixtureCapture<F>
    where
        F: Fn(&dyn Fn() -> bool) -> Result<RgbaImage, String> + Send + Sync,
    {
        fn capture(
            &self,
            _: &CaptureTarget,
            cancelled: &dyn Fn() -> bool,
        ) -> Result<RgbaImage, String> {
            (self.0)(cancelled)
        }
    }
    struct FixtureNaming<F>(F);
    impl<F> NamingAdapter for FixtureNaming<F>
    where
        F: Fn(&RgbaImage) -> Result<Option<String>, String> + Send + Sync,
    {
        fn suggest(&self, image: &RgbaImage) -> Result<Option<String>, String> {
            (self.0)(image)
        }
    }
    fn config(limit: u64) -> RunConfig {
        RunConfig {
            target: CaptureTarget {
                display: 1,
                region: crate::editor::Rect {
                    x: 0,
                    y: 0,
                    width: 16,
                    height: 9,
                },
                source_width: 16,
                source_height: 9,
            },
            schedule: CaptureSchedule::Manual,
            interval: Duration::from_secs(10),
            change: ChangeConfig::default(),
            auto_name: true,
            max_captures: limit,
            max_duration: Duration::from_secs(30),
        }
    }
    fn pixels(value: u8) -> RgbaImage {
        RgbaImage::from_pixel(16, 9, image::Rgba([value, value, value, 255]))
    }
    fn await_event(
        runner: &SessionRunner,
        accepts: impl Fn(&SessionEvent) -> bool,
    ) -> SessionEvent {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let event = runner
                .events
                .recv_timeout(remaining)
                .expect("worker event before deadline");
            if accepts(&event) {
                return event;
            }
        }
    }

    #[test]
    fn stopped_phase_does_not_mean_drained_during_png_persistence() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = SessionStore::create(directory.path(), "drain persistence").unwrap();
        let (entered, waiting) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        store.test_persistence_gate(entered, gate);
        let mut settings = config(10);
        settings.auto_name = false;
        let runner = SessionRunner::start_with_adapters(
            store,
            settings,
            WorkerAdapters {
                capture: Arc::new(FixtureCapture(|_: &dyn Fn() -> bool| Ok(pixels(90)))),
                naming: Arc::new(FixtureNaming(|_: &RgbaImage| Ok(None))),
            },
            || {},
        );
        runner.capture_now().unwrap();
        waiting.recv_timeout(Duration::from_secs(3)).unwrap();
        runner.stop();
        assert!(runner.is_stopped());
        assert!(!runner.is_drained());
        assert!(runner.events.try_recv().is_err());
        release.send(()).unwrap();
        let SessionEvent::Saved(handle, _) =
            await_event(&runner, |event| matches!(event, SessionEvent::Saved(..)))
        else {
            unreachable!()
        };
        await_event(&runner, |event| matches!(event, SessionEvent::Finished(_)));
        assert!(runner.is_drained());
        assert!(handle.snapshot().unwrap().path.is_file());
        assert_eq!(handle.load().unwrap().0, pixels(90));
    }
    #[test]
    fn drained_waits_for_pending_naming_and_its_metadata_update() {
        let directory = tempfile::tempdir().unwrap();
        let store = SessionStore::create(directory.path(), "drain naming").unwrap();
        let (entered, waiting) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let gate = std::sync::Mutex::new(gate);
        let runner = SessionRunner::start_with_adapters(
            store,
            config(10),
            WorkerAdapters {
                capture: Arc::new(FixtureCapture(|_: &dyn Fn() -> bool| Ok(pixels(90)))),
                naming: Arc::new(FixtureNaming(move |_: &RgbaImage| {
                    entered.send(()).unwrap();
                    gate.lock().unwrap().recv().unwrap();
                    Ok(Some("Finished name".into()))
                })),
            },
            || {},
        );
        runner.capture_now().unwrap();
        let SessionEvent::Saved(handle, _) =
            await_event(&runner, |event| matches!(event, SessionEvent::Saved(..)))
        else {
            unreachable!()
        };
        waiting.recv_timeout(Duration::from_secs(3)).unwrap();
        runner.stop();
        assert!(runner.is_stopped());
        assert!(!runner.is_drained());
        release.send(()).unwrap();
        await_event(&runner, |event| matches!(event, SessionEvent::Finished(_)));
        assert!(runner.is_drained());
        assert_eq!(handle.snapshot().unwrap().title, "Finished name");
        assert!(handle.metadata_warning().is_none());
    }

    #[test]
    fn worker_acknowledges_durable_png_before_blocked_naming_finishes() {
        let directory = tempfile::tempdir().unwrap();
        let store = SessionStore::create(directory.path(), "worker").unwrap();
        let (release, gate) = mpsc::channel();
        let gate = std::sync::Mutex::new(gate);
        let runner = SessionRunner::start_with_adapters(
            store,
            config(1),
            WorkerAdapters {
                capture: Arc::new(FixtureCapture(|_: &dyn Fn() -> bool| Ok(pixels(80)))),
                naming: Arc::new(FixtureNaming(move |_: &RgbaImage| {
                    gate.lock().unwrap().recv().unwrap();
                    Ok(Some("Synthetic slide".into()))
                })),
            },
            || {},
        );
        runner.capture_now().unwrap();
        let SessionEvent::Saved(handle, _) =
            await_event(&runner, |event| matches!(event, SessionEvent::Saved(..)))
        else {
            unreachable!()
        };
        let saved = handle.snapshot().unwrap();
        assert_eq!(crate::editor::load_image(&saved.path).unwrap(), pixels(80));
        release.send(()).unwrap();
        let SessionEvent::Named { old_path, capture } =
            await_event(&runner, |event| matches!(event, SessionEvent::Named { .. }))
        else {
            unreachable!()
        };
        assert_eq!(old_path, saved.path);
        assert!(capture.path.is_file());
        assert!(!old_path.exists());
        assert_eq!(
            crate::editor::load_image(&capture.path).unwrap(),
            pixels(80)
        );
    }

    #[test]
    fn worker_discards_in_flight_old_generation_and_bounds_manual_queue() {
        let directory = tempfile::tempdir().unwrap();
        let store = SessionStore::create(directory.path(), "generation").unwrap();
        let (entered, waiting) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let gate = std::sync::Mutex::new(gate);
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let runner = SessionRunner::start_with_adapters(
            store,
            config(1),
            WorkerAdapters {
                capture: Arc::new(FixtureCapture(move |_: &dyn Fn() -> bool| {
                    let call = calls.fetch_add(1, Ordering::SeqCst);
                    if call == 0 {
                        entered.send(()).unwrap();
                        gate.lock().unwrap().recv().unwrap();
                    }
                    Ok(pixels(if call == 0 { 20 } else { 90 }))
                })),
                naming: Arc::new(FixtureNaming(|_: &RgbaImage| Ok(None))),
            },
            || {},
        );
        runner.capture_now().unwrap();
        waiting.recv_timeout(Duration::from_secs(3)).unwrap();
        for _ in 0..8 {
            runner.capture_now().unwrap();
        }
        assert!(runner.capture_now().unwrap_err().contains("busy"));
        runner.pause();
        runner.resume();
        release.send(()).unwrap();
        // The queue still contains only old-generation requests. Wait for its
        // bounded drain by retrying the new request, without an arbitrary sleep.
        let deadline = Instant::now() + Duration::from_secs(3);
        while runner.capture_now().is_err() {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        let SessionEvent::Saved(handle, _) =
            await_event(&runner, |event| matches!(event, SessionEvent::Saved(..)))
        else {
            unreachable!()
        };
        assert_eq!(
            crate::editor::load_image(&handle.snapshot().unwrap().path).unwrap(),
            pixels(90)
        );
        assert_eq!(handle.snapshot().unwrap().sequence, 1);
    }

    #[test]
    fn stop_cancels_delayed_capture_without_acknowledging_or_resurrecting_it() {
        let directory = tempfile::tempdir().unwrap();
        let store = SessionStore::create(directory.path(), "cancel").unwrap();
        let (entered, waiting) = mpsc::channel();
        let runner = SessionRunner::start_with_adapters(
            store,
            config(1),
            WorkerAdapters {
                capture: Arc::new(FixtureCapture(move |cancelled: &dyn Fn() -> bool| {
                    entered.send(()).unwrap();
                    while !cancelled() {
                        std::thread::yield_now();
                    }
                    Err("cancelled".into())
                })),
                naming: Arc::new(FixtureNaming(|_: &RgbaImage| Ok(None))),
            },
            || {},
        );
        runner.capture_now().unwrap();
        waiting.recv_timeout(Duration::from_secs(3)).unwrap();
        runner.stop();
        assert!(matches!(
            runner.events.recv_timeout(Duration::from_secs(2)).unwrap(),
            SessionEvent::Finished(_)
        ));
        assert!(runner.is_stopped());
        runner.resume();
        assert!(runner.is_stopped());
        assert!(runner.capture_now().is_err());
        assert!(
            !std::fs::read_dir(&runner.folder).unwrap().any(|entry| entry
                .unwrap()
                .path()
                .extension()
                .is_some_and(|ext| ext == "png"))
        );
    }

    #[test]
    fn persistence_failure_pauses_without_a_saved_acknowledgement() {
        let directory = tempfile::tempdir().unwrap();
        let store = SessionStore::create(directory.path(), "write failure").unwrap();
        let folder = store.folder().to_path_buf();
        let (entered, waiting) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let gate = std::sync::Mutex::new(gate);
        let runner = SessionRunner::start_with_adapters(
            store,
            config(1),
            WorkerAdapters {
                capture: Arc::new(FixtureCapture(move |_: &dyn Fn() -> bool| {
                    entered.send(()).unwrap();
                    gate.lock().unwrap().recv().unwrap();
                    Ok(pixels(80))
                })),
                naming: Arc::new(FixtureNaming(|_: &RgbaImage| Ok(None))),
            },
            || {},
        );
        runner.capture_now().unwrap();
        waiting.recv_timeout(Duration::from_secs(3)).unwrap();
        std::fs::remove_dir_all(&folder).unwrap();
        std::fs::write(&folder, b"destination replaced by a file").unwrap();
        release.send(()).unwrap();
        assert!(
            matches!(runner.events.recv_timeout(Duration::from_secs(2)).unwrap(),SessionEvent::Paused(reason) if reason.contains("Saving failed"))
        );
        assert!(runner.is_paused());
        assert!(runner.capture_now().is_err());
        // Restore the destination and explicitly retry. A failed write must
        // not consume the first sequence number or acknowledge an image.
        std::fs::remove_file(&folder).unwrap();
        std::fs::create_dir(&folder).unwrap();
        runner.resume();
        release.send(()).unwrap();
        runner.capture_now().unwrap();
        let SessionEvent::Saved(handle, _) =
            await_event(&runner, |event| matches!(event, SessionEvent::Saved(..)))
        else {
            unreachable!()
        };
        assert_eq!(handle.snapshot().unwrap().sequence, 1);
        assert!(handle.snapshot().unwrap().path.is_file());
        runner.stop();
    }

    #[test]
    fn slow_and_failing_naming_never_blocks_persistence_or_loses_frames() {
        let directory = tempfile::tempdir().unwrap();
        let store = SessionStore::create(directory.path(), "naming queue").unwrap();
        let (release, gate) = mpsc::channel();
        let gate = std::sync::Mutex::new(gate);
        let (entered, waiting) = mpsc::channel();
        let once = std::sync::atomic::AtomicBool::new(false);
        let runner = SessionRunner::start_with_adapters(
            store,
            config(7),
            WorkerAdapters {
                capture: Arc::new(FixtureCapture(|_: &dyn Fn() -> bool| Ok(pixels(80)))),
                naming: Arc::new(FixtureNaming(move |_: &RgbaImage| {
                    if !once.swap(true, Ordering::SeqCst) {
                        entered.send(()).unwrap();
                        gate.lock().unwrap().recv().unwrap();
                    }
                    Err("synthetic recogniser failure".into())
                })),
            },
            || {},
        );
        let mut paths = Vec::new();
        let mut busy = 0;
        for index in 0..7 {
            runner.capture_now().unwrap();
            loop {
                match runner.events.recv_timeout(Duration::from_secs(3)).unwrap() {
                    SessionEvent::Saved(handle, _) => {
                        paths.push(handle.snapshot().unwrap().path);
                        break;
                    }
                    SessionEvent::Note(note) if note.contains("queue is busy") => busy += 1,
                    _ => {}
                }
            }
            if index == 0 {
                waiting.recv_timeout(Duration::from_secs(3)).unwrap();
            }
        }
        // The final Saved event precedes enqueueing its OCR work. Wait until
        // the limit stops capture before releasing the recogniser barrier.
        let deadline = Instant::now() + Duration::from_secs(3);
        while !runner.is_stopped() {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(!runner.is_drained());
        assert!(paths.iter().all(|path| path.is_file()));
        release.send(()).unwrap();
        let mut failed_naming = false;
        loop {
            match runner.events.recv_timeout(Duration::from_secs(3)).unwrap() {
                SessionEvent::Finished(_) => break,
                SessionEvent::Note(note) if note.contains("queue is busy") => busy += 1,
                SessionEvent::Note(note) if note.contains("synthetic recogniser failure") => {
                    failed_naming = true
                }
                _ => {}
            }
        }
        assert_eq!(busy, 2);
        assert!(failed_naming);
        assert!(runner.is_drained());
        assert!(paths.iter().all(|path| path.is_file()));
    }

    #[test]
    fn manual_request_in_change_mode_bypasses_settle_and_resume_resets_baseline() {
        let directory = tempfile::tempdir().unwrap();
        let store = SessionStore::create(directory.path(), "resume baseline").unwrap();
        let mut settings = config(2);
        settings.schedule = CaptureSchedule::Change;
        settings.auto_name = false;
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let capture_count = count.clone();
        let runner = SessionRunner::start_with_adapters(
            store,
            settings,
            WorkerAdapters {
                capture: Arc::new(FixtureCapture(move |_: &dyn Fn() -> bool| {
                    capture_count.fetch_add(1, Ordering::SeqCst);
                    Ok(pixels(80))
                })),
                naming: Arc::new(FixtureNaming(|_: &RgbaImage| Ok(None))),
            },
            || {},
        );
        runner.capture_now().unwrap();
        let SessionEvent::Saved(first, _) =
            await_event(&runner, |event| matches!(event, SessionEvent::Saved(..)))
        else {
            unreachable!()
        };
        assert_eq!(count.load(Ordering::SeqCst), 1);
        runner.pause();
        runner.resume();
        let SessionEvent::Saved(second, _) =
            await_event(&runner, |event| matches!(event, SessionEvent::Saved(..)))
        else {
            unreachable!()
        };
        assert_eq!(count.load(Ordering::SeqCst), 3);
        assert_eq!(first.load().unwrap().0, second.load().unwrap().0);
    }

    /// Reproducible corpus: 30 intended labels, with cursor/video nuisance,
    /// single-sample transitions, text-only edits, isoluminant colour edits,
    /// returned slides, aspect changes and rapid frames. No desktop is read.
    fn benchmark_corpus() -> Vec<RgbaImage> {
        let labelled = |id: u8, base: [u8; 4], dimensions: (u32, u32)| {
            let mut image = RgbaImage::from_pixel(dimensions.0, dimensions.1, image::Rgba(base));
            image.put_pixel(0, 0, image::Rgba([id, 0, 0, 255]));
            image
        };
        let mut frames = Vec::new();
        for id in 0..20 {
            let intensity = if id % 2 == 0 { 40 } else { 180 };
            let dimensions = if id % 5 == 0 { (96, 72) } else { (128, 72) };
            frames.push(labelled(255, [255, 255, 255, 255], dimensions));
            for sample in 0..3 {
                let mut image = labelled(id, [intensity, intensity, intensity, 255], dimensions);
                // Small moving cursor/video tile occupy less than 4% of area.
                for x in 20 + sample * 3..23 + sample * 3 {
                    for y in 20..23 {
                        image.put_pixel(x, y, image::Rgba([0, 0, 0, 255]));
                    }
                }
                frames.push(image);
            }
        }
        for id in 20..24 {
            let mut image = labelled(id, [180, 180, 180, 255], (128, 72));
            for x in 10..12 {
                for y in 5..15 + u32::from(id - 20) * 5 {
                    image.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
                }
            }
            frames.extend([image.clone(), image.clone(), image]);
        }
        for id in 24..27 {
            let colour = if id == 25 {
                [0, 131, 0, 255]
            } else {
                [255, 0, 0, 255]
            };
            let image = labelled(id, colour, (128, 72));
            frames.extend([image.clone(), image.clone(), image]);
        }
        for id in 27..30 {
            frames.push(labelled(id, [id * 7, id * 7, id * 7, 255], (128, 72)));
        }
        frames
    }

    #[test]
    #[ignore = "Three parallel real-worker corpus runs take about two minutes; no native capture"]
    fn synthetic_change_mode_benchmark_three_runs() {
        let results = std::thread::scope(|scope| {
            let tasks=(1..=3).map(|repeat|scope.spawn(move || {
                let frames=benchmark_corpus();let total=frames.len();
                let sample_times=Arc::new(std::sync::Mutex::new(Vec::new()));
                let adapter_times=sample_times.clone();
                let position=std::sync::atomic::AtomicUsize::new(0);
                let directory=tempfile::tempdir().unwrap();
                let store=SessionStore::create(directory.path(),"synthetic corpus").unwrap();
                let mut settings=config(1000);settings.auto_name=false;settings.schedule=CaptureSchedule::Change;settings.max_duration=Duration::from_secs(180);
                let runner=SessionRunner::start_with_adapters(store,settings,WorkerAdapters {
                    capture:Arc::new(FixtureCapture(move |_:&dyn Fn()->bool| {
                        let index=position.fetch_add(1,Ordering::SeqCst);
                        let image=frames.get(index).cloned().ok_or("Synthetic corpus complete")?;
                        adapter_times.lock().unwrap().push((image.get_pixel(0,0)[0],Instant::now()));Ok(image)
                    })),
                    naming:Arc::new(FixtureNaming(|_:&RgbaImage|Ok(None))),
                },||{});
                let mut saved=[0u32;30];let mut transitions=0;let mut latencies=Vec::new();
                loop {
                    match runner.events.recv_timeout(Duration::from_secs(45)).expect("corpus progressing") {
                        SessionEvent::Saved(handle,_)=>{
                            let (image,_)=handle.load().unwrap();let id=image.get_pixel(0,0)[0];
                            if id==255 {transitions+=1;} else {
                                saved[id as usize]+=1;
                                let times=sample_times.lock().unwrap();
                                let first=times.iter().find(|(label,_)|*label==id).unwrap().1;
                                latencies.push(first.elapsed().as_millis());
                            }
                        },
                        SessionEvent::Paused(reason)=>{assert_eq!(reason,"Synthetic corpus complete");runner.stop();break;},
                        SessionEvent::Finished(reason)=>panic!("Unexpected completion: {reason}"),
                        _=>{}
                    }
                }
                let missed:Vec<_>=saved.iter().enumerate().filter(|(_,count)|**count==0).map(|(index,_)|index).collect();
                let duplicates:u32=saved.iter().map(|count|count.saturating_sub(1)).sum();
                println!("CORPUS repeat={repeat} samples={total} intended=30 saved={} missed={missed:?} duplicate={duplicates} transitional={transitions} latency_min_ms={} latency_max_ms={}",saved.iter().sum::<u32>(),latencies.iter().min().unwrap(),latencies.iter().max().unwrap());
                (missed,duplicates,transitions)
            })).collect::<Vec<_>>();
            tasks
                .into_iter()
                .map(|task| task.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(results[0], results[1]);
        assert_eq!(results[1], results[2]);
        assert_eq!(results[0].0, vec![20, 21, 22, 23, 25, 26, 27, 29]);
        assert_eq!((results[0].1, results[0].2), (0, 0));
    }

    fn at(milliseconds: u64) -> Duration {
        Duration::from_millis(milliseconds)
    }

    #[test]
    fn stopped_state_cannot_be_resurrected_by_an_in_flight_error() {
        let state = RunState::new();
        let capture = state.snapshot();
        state.stop();
        let stopped = state.snapshot();

        assert!(!state.pause_if_current(capture));
        assert!(!state.transition(RUNNING, PAUSED));
        assert!(!state.transition(PAUSED, RUNNING));
        state.stop();
        assert_eq!(state.snapshot(), stopped);
        assert_eq!(stopped.phase(), STOPPED);
    }

    #[test]
    fn rapid_pause_resume_invalidates_capture_and_error_snapshots() {
        let state = RunState::new();
        let before = state.snapshot();
        assert!(state.transition(RUNNING, PAUSED));
        assert!(state.transition(PAUSED, RUNNING));
        let resumed = state.snapshot();

        assert_eq!(resumed.phase(), RUNNING);
        assert_ne!(resumed.generation(), before.generation());
        assert!(!state.is_current_running(before));
        assert!(!state.pause_if_current(before));
        assert!(state.is_current_running(resumed));
    }

    #[test]
    fn current_error_pauses_once_and_invalidates_queued_work() {
        let state = RunState::new();
        let capture = state.snapshot();
        assert!(state.pause_if_current(capture));
        assert!(!state.pause_if_current(capture));
        assert_eq!(state.snapshot().phase(), PAUSED);
        assert_ne!(state.snapshot().generation(), capture.generation());
    }

    #[test]
    fn manual_capture_obeys_arming_but_ignores_a_long_interval() {
        let mut clock = CaptureClock::new(0, at(0));
        clock.sampled(CaptureSchedule::Interval, Duration::from_secs(600), at(0));
        clock.request_manual(0);

        assert!(!clock.ready(CaptureSchedule::Interval, at(699)));
        assert!(clock.ready(CaptureSchedule::Interval, at(700)));
        clock.pending_manual = false;
        assert!(!clock.ready(CaptureSchedule::Interval, at(701)));
    }

    #[test]
    fn missed_pause_still_rearms_and_discards_pending_old_hotkeys() {
        let state = RunState::new();
        let first = state.snapshot();
        let mut clock = CaptureClock::new(first.generation(), at(0));
        clock.request_manual(first.generation());
        state.transition(RUNNING, PAUSED);
        state.transition(PAUSED, RUNNING);
        let resumed = state.snapshot();

        // The worker only sees RUNNING snapshots before and after the pause.
        assert!(clock.synchronize(resumed.generation(), at(2000)));
        assert!(!clock.pending_manual);
        clock.request_manual(first.generation());
        assert!(!clock.pending_manual);
        clock.request_manual(resumed.generation());
        assert!(!clock.ready(CaptureSchedule::Manual, at(2699)));
        assert!(clock.ready(CaptureSchedule::Manual, at(2700)));
    }

    #[test]
    fn polling_same_generation_does_not_restart_arming() {
        let mut clock = CaptureClock::new(4, at(100));
        assert!(!clock.synchronize(4, at(799)));
        assert!(!clock.ready(CaptureSchedule::Interval, at(799)));
        assert!(clock.ready(CaptureSchedule::Interval, at(800)));
    }

    #[test]
    fn manual_schedule_never_captures_without_a_current_request() {
        let mut clock = CaptureClock::new(3, at(0));
        clock.request_manual(2);
        assert!(!clock.ready(CaptureSchedule::Manual, Duration::from_secs(60)));
        clock.request_manual(3);
        assert!(clock.ready(CaptureSchedule::Manual, Duration::from_secs(60)));
    }

    #[test]
    fn reaching_a_limit_emits_one_finished_event_with_the_limit_reason() {
        let directory = tempfile::tempdir().unwrap();
        let store = SessionStore::create(directory.path(), "limit test").unwrap();
        let runner = SessionRunner::start(
            store,
            RunConfig {
                target: CaptureTarget {
                    display: 1,
                    region: crate::editor::Rect {
                        x: 0,
                        y: 0,
                        width: 1,
                        height: 1,
                    },
                    source_width: 1,
                    source_height: 1,
                },
                schedule: CaptureSchedule::Manual,
                interval: Duration::from_secs(10),
                change: ChangeConfig::default(),
                auto_name: false,
                max_captures: 0,
                max_duration: Duration::from_secs(60),
            },
            || {},
        );
        let event = runner.events.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(
            matches!(event, SessionEvent::Finished(reason) if reason.starts_with("Session limit reached."))
        );
        assert!(matches!(
            runner.events.recv_timeout(Duration::from_secs(2)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
        assert!(runner.is_stopped());
    }
}
