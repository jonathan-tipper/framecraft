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
        atomic::{AtomicU64, Ordering},
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
        let state = Arc::new(RunState::new());
        let (commands, rx) = mpsc::sync_channel(8);
        let (tx, events) = mpsc::channel();
        let folder = store.folder().to_path_buf();
        let worker_state = state.clone();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
        std::thread::spawn(move || run(store, config, worker_state, rx, tx, wake));
        Self {
            state,
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
    state: Arc<RunState>,
    commands: mpsc::Receiver<u64>,
    events: mpsc::Sender<SessionEvent>,
    wake: Arc<dyn Fn() + Send + Sync>,
) {
    let (names, name_rx) = mpsc::sync_channel::<(CaptureHandle, SavedCapture, RgbaImage)>(4);
    let name_events = events.clone();
    let name_wake = wake.clone();
    let naming_enabled = config.auto_name;
    std::thread::spawn(move || {
        for (handle, original, pixels) in name_rx {
            match naming::suggest(&pixels) {
                Ok(Some(suggestion)) => {
                    match handle.rename_if_unchanged(&original, &suggestion.title) {
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
                    }
                }
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
        let pixels = match config.target.capture() {
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
                    let handle = CaptureHandle::new(saved.clone());
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
    send(&events, &wake, SessionEvent::Finished(finish_reason.into()));
}

#[cfg(test)]
mod tests {
    use super::*;

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
