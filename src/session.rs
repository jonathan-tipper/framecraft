//! Screenshot-session persistence and visual change detection, independent of UI.
//!
//! All filename timestamps are UTC. Session folders and PNGs are created without
//! replacing existing files. Only flattened RGBA pixels are written; OCR text,
//! source metadata and editable layers are never saved as sidecars.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use image::imageops::FilterType;
use image::{ExtendedColorType, ImageEncoder, RgbaImage};
use tempfile::NamedTempFile;

const MAX_COLLISIONS: usize = 10_000;
const THUMB_WIDTH: u32 = 128;
const THUMB_HEIGHT: u32 = 72;
const PIXEL_DIFFERENCE: u8 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    Manual,
    Interval,
    Change,
}

impl Trigger {
    pub fn label(self) -> &'static str {
        match self {
            Self::Manual => "Manual",
            Self::Interval => "Interval",
            Self::Change => "Change",
        }
    }

    fn default_title(self) -> &'static str {
        match self {
            Self::Manual => "Manual capture",
            Self::Interval => "Interval capture",
            Self::Change => "Visual change",
        }
    }
}

#[derive(Clone, Debug)]
pub struct SavedCapture {
    pub path: PathBuf,
    pub title: String,
    pub sequence: u64,
}

/// One shared identity for a saved frame, even while its filename changes.
/// Clones share a per-capture lock; no other capture is blocked by a rename.
#[derive(Clone, Debug)]
pub struct CaptureHandle {
    inner: Arc<Mutex<CaptureState>>,
}

#[derive(Debug)]
struct CaptureState {
    capture: SavedCapture,
    manually_named: bool,
}

impl CaptureHandle {
    pub(crate) fn new(capture: SavedCapture) -> Self {
        Self {
            inner: Arc::new(Mutex::new(CaptureState {
                capture,
                manually_named: false,
            })),
        }
    }

    fn lock(&self) -> Result<MutexGuard<'_, CaptureState>, String> {
        self.inner
            .lock()
            .map_err(|_| "The capture could not be accessed after an interrupted operation.".into())
    }

    /// Metadata is a snapshot for display, never a path to open without the handle.
    pub fn snapshot(&self) -> Result<SavedCapture, String> {
        Ok(self.lock()?.capture.clone())
    }

    /// A successful explicit rename takes precedence over pending OCR naming,
    /// including when the user intentionally keeps the current name.
    pub fn rename(&self, title: &str) -> Result<SavedCapture, String> {
        let mut state = self.lock()?;
        let renamed = rename_capture(&state.capture, title)?;
        state.capture = renamed.clone();
        state.manually_named = true;
        Ok(renamed)
    }

    /// Resolve and decode under the same lock used by renames. Run this method
    /// on a background worker; the returned path is only a naming suggestion.
    pub fn load(&self) -> Result<(RgbaImage, PathBuf), String> {
        self.load_with(crate::editor::load_image)
    }

    fn load_with(
        &self,
        decode: impl FnOnce(&Path) -> Result<RgbaImage, String>,
    ) -> Result<(RgbaImage, PathBuf), String> {
        let state = self.lock()?;
        let pixels = decode(&state.capture.path)?;
        Ok((pixels, state.capture.path.clone()))
    }

    /// OCR runs without a lock, then conditionally applies its result. Stale
    /// results and manual names are skipped rather than treated as failures.
    pub(crate) fn rename_if_unchanged(
        &self,
        original: &SavedCapture,
        title: &str,
    ) -> Result<Option<SavedCapture>, String> {
        let mut state = self.lock()?;
        if state.manually_named
            || state.capture.path != original.path
            || state.capture.title != original.title
            || state.capture.sequence != original.sequence
        {
            return Ok(None);
        }
        let renamed = rename_capture(&state.capture, title)?;
        state.capture = renamed.clone();
        Ok(Some(renamed))
    }
}

#[derive(Debug)]
pub struct SessionStore {
    folder: PathBuf,
    sequence: u64,
}

impl SessionStore {
    /// Create a new session without reusing an existing directory.
    pub fn create(base: &Path, title: &str) -> Result<Self, String> {
        fs::create_dir_all(base)
            .map_err(|e| format!("Could not create the session destination: {e}"))?;
        let stem = format!("{}-{}", timestamp_now()?, slug(title));
        for attempt in 0..MAX_COLLISIONS {
            let folder = base.join(with_collision_suffix(&stem, attempt));
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&folder) {
                Ok(()) => {
                    sync_directory(base).map_err(|e| {
                        let _ = fs::remove_dir(&folder);
                        format!("Could not sync the new session directory: {e}")
                    })?;
                    return Ok(Self {
                        folder,
                        sequence: 0,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(format!("Could not create the session directory: {error}"));
                }
            }
        }
        Err("There are too many sessions with the same name. Choose another title.".into())
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// Atomically persist a PNG, then acknowledge its sequence number.
    /// A failed save never advances the store or overwrites another capture.
    pub fn save(
        &mut self,
        pixels: &RgbaImage,
        title: Option<&str>,
        trigger: Trigger,
    ) -> Result<SavedCapture, String> {
        check_image(pixels)?;
        let title = title
            .map(display_title)
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| trigger.default_title().to_string());
        let timestamp = timestamp_now()?;
        let mut temporary = NamedTempFile::new_in(&self.folder)
            .map_err(|e| format!("Could not prepare a capture file: {e}"))?;
        image::codecs::png::PngEncoder::new(temporary.as_file_mut())
            .write_image(
                pixels.as_raw(),
                pixels.width(),
                pixels.height(),
                ExtendedColorType::Rgba8,
            )
            .map_err(|e| format!("Could not encode the capture: {e}"))?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|e| format!("Could not sync the capture to disk: {e}"))?;

        let mut sequence = self.sequence;
        for _ in 0..MAX_COLLISIONS {
            sequence = sequence
                .checked_add(1)
                .ok_or("The capture sequence is exhausted.")?;
            let path = self
                .folder
                .join(format!("{timestamp}-{sequence:04}-{}.png", slug(&title)));
            match temporary.persist_noclobber(&path) {
                Ok(_file) => {
                    if let Err(error) = sync_directory(&self.folder) {
                        let _ = fs::remove_file(&path);
                        return Err(format!(
                            "Could not sync the saved capture directory: {error}"
                        ));
                    }
                    self.sequence = sequence;
                    return Ok(SavedCapture {
                        path,
                        title,
                        sequence,
                    });
                }
                Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => {
                    temporary = error.file;
                }
                Err(error) => {
                    return Err(format!("Could not save the capture: {}", error.error));
                }
            }
        }
        Err("There are too many conflicting capture filenames.".into())
    }
}

/// Change a capture's title without replacing any existing file.
/// Timestamp and sequence remain intact; new names stay in the same directory.
/// A blank title is a no-op. Hard linking keeps the PNG bytes unchanged.
pub fn rename_capture(capture: &SavedCapture, new_title: &str) -> Result<SavedCapture, String> {
    let title = display_title(new_title);
    if title.is_empty() {
        return Ok(capture.clone());
    }
    let parent = capture
        .path
        .parent()
        .ok_or("The capture has no parent directory.")?;
    let filename = capture
        .path
        .file_stem()
        .and_then(|name| name.to_str())
        .ok_or("The capture filename is not valid UTF-8.")?;
    let mut components = filename.splitn(4, '-');
    let date = components.next().unwrap_or("");
    let time = components.next().unwrap_or("");
    let sequence = components.next().unwrap_or("");
    if date.len() != 8
        || time.len() != 6
        || !date.bytes().all(|b| b.is_ascii_digit())
        || !time.bytes().all(|b| b.is_ascii_digit())
        || sequence.parse::<u64>().ok() != Some(capture.sequence)
        || components.next().is_none()
    {
        return Err("The capture does not have a recognised session filename.".into());
    }
    let metadata = fs::symlink_metadata(&capture.path)
        .map_err(|e| format!("Could not inspect the capture: {e}"))?;
    if !metadata.file_type().is_file() {
        return Err("Only regular capture files can be renamed.".into());
    }
    let stem = format!("{date}-{time}-{sequence}-{}", slug(&title));
    for attempt in 0..MAX_COLLISIONS {
        let path = parent.join(format!("{}.png", with_collision_suffix(&stem, attempt)));
        if path == capture.path {
            return Ok(SavedCapture {
                path,
                title,
                sequence: capture.sequence,
            });
        }
        match fs::hard_link(&capture.path, &path) {
            Ok(()) => {
                if let Err(error) = sync_directory(parent) {
                    let _ = fs::remove_file(&path);
                    return Err(format!("Could not sync the renamed capture: {error}"));
                }
                if let Err(error) = fs::remove_file(&capture.path) {
                    let _ = fs::remove_file(&path);
                    return Err(format!("Could not remove the old capture name: {error}"));
                }
                if let Err(error) = sync_directory(parent) {
                    // Restore the old link if a durability check failed. Never
                    // remove the new name unless the old name was restored.
                    if fs::hard_link(&path, &capture.path).is_ok() {
                        let _ = fs::remove_file(&path);
                        return Err(format!("Could not sync the capture rename: {error}"));
                    }
                    // The new name is the only known surviving copy. Return it
                    // so callers never lose the location of their saved PNG.
                }
                return Ok(SavedCapture {
                    path,
                    title,
                    sequence: capture.sequence,
                });
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("Could not rename the capture: {error}")),
        }
    }
    Err("There are too many captures with the requested title.".into())
}

fn check_image(image: &RgbaImage) -> Result<(), String> {
    let (width, height) = image.dimensions();
    if width == 0
        || height == 0
        || width > 16_384
        || height > 16_384
        || u64::from(width) * u64::from(height) > 64_000_000
    {
        return Err("Captures must be non-empty, at most 16,384 pixels per edge and 64 million pixels total.".into());
    }
    Ok(())
}

fn display_title(title: &str) -> String {
    let normalised: String = title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .take(120)
        .collect();
    normalised.trim().to_string()
}

fn slug(title: &str) -> String {
    let mut output = String::new();
    let mut separator = false;
    for ch in title.chars() {
        if ch.is_alphanumeric() {
            let lower: String = ch.to_lowercase().collect();
            let added = lower.len() + usize::from(separator && !output.is_empty());
            if output.len() + added > 56 {
                break;
            }
            if separator && !output.is_empty() {
                output.push('-');
            }
            output.push_str(&lower);
            separator = false;
        } else {
            separator = true;
        }
    }
    if output.is_empty() {
        "capture".into()
    } else {
        output
    }
}

fn with_collision_suffix(stem: &str, attempt: usize) -> String {
    if attempt == 0 {
        stem.into()
    } else {
        format!("{stem}-{:02}", attempt + 1)
    }
}

fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(path)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        // File::open cannot portably open directories on Windows. The PNG
        // itself is still synced before the atomic persistence operation.
        let _ = path;
        Ok(())
    }
}

fn timestamp_now() -> Result<String, String> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "The system clock is before 1970. Correct it before capturing.".to_string())?
        .as_secs();
    Ok(timestamp_utc(seconds))
}

/// Gregorian civil date conversion from Unix days, using 400-year eras.
fn timestamp_utc(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64 + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    let remainder = seconds % 86_400;
    format!(
        "{year:04}{month:02}{day:02}-{:02}{:02}{:02}",
        remainder / 3600,
        (remainder % 3600) / 60,
        remainder % 60
    )
}

#[derive(Clone, Debug)]
pub struct ChangeConfig {
    /// Fraction of thumbnail pixels changing by at least 16/255 intensity.
    /// Values are clamped to 0.005..=1.0; non-finite values use the default.
    pub threshold: f32,
    pub settle: Duration,
    pub cooldown: Duration,
}

impl Default for ChangeConfig {
    fn default() -> Self {
        Self {
            threshold: 0.04,
            settle: Duration::from_millis(800),
            cooldown: Duration::from_secs(2),
        }
    }
}

#[derive(Clone, Debug)]
struct Fingerprint {
    dimensions: (u32, u32),
    grey: Vec<u8>,
}

impl Fingerprint {
    fn from_image(image: &RgbaImage) -> Self {
        let thumbnail =
            image::imageops::resize(image, THUMB_WIDTH, THUMB_HEIGHT, FilterType::Triangle);
        let grey = thumbnail
            .pixels()
            .map(|pixel| {
                // Composite transparency against black before measuring light.
                let luma = (77 * u32::from(pixel[0])
                    + 150 * u32::from(pixel[1])
                    + 29 * u32::from(pixel[2])
                    + 128)
                    >> 8;
                ((luma * u32::from(pixel[3]) + 127) / 255) as u8
            })
            .collect();
        Self {
            dimensions: image.dimensions(),
            grey,
        }
    }

    fn difference(&self, other: &Self) -> f32 {
        if self.dimensions != other.dimensions {
            return 1.0;
        }
        let changed = self
            .grey
            .iter()
            .zip(&other.grey)
            .filter(|(a, b)| a.abs_diff(**b) >= PIXEL_DIFFERENCE)
            .count();
        changed as f32 / self.grey.len() as f32
    }
}

#[derive(Clone, Debug)]
struct Candidate {
    fingerprint: Fingerprint,
    since: Duration,
}

#[derive(Debug)]
pub struct ChangeDetector {
    config: ChangeConfig,
    candidate: Option<Candidate>,
    last_saved: Option<Fingerprint>,
    last_saved_at: Option<Duration>,
    last_observed_at: Option<Duration>,
}

impl ChangeDetector {
    pub fn new(mut config: ChangeConfig) -> Self {
        config.threshold = if config.threshold.is_finite() {
            config.threshold.clamp(0.005, 1.0)
        } else {
            ChangeConfig::default().threshold
        };
        Self {
            config,
            candidate: None,
            last_saved: None,
            last_saved_at: None,
            last_observed_at: None,
        }
    }

    /// Return true when the current image is a settled, significant change.
    /// `now` is monotonic elapsed time. Small cursor moves and animations below
    /// the configured area threshold do not restart the settling timer.
    /// This never updates the saved baseline: call mark_saved only after disk
    /// persistence succeeds. A failed save remains eligible for a later retry.
    pub fn observe(&mut self, image: &RgbaImage, now: Duration) -> bool {
        if check_image(image).is_err() {
            return false;
        }
        let fingerprint = Fingerprint::from_image(image);
        if self.last_observed_at.is_some_and(|last| now < last) {
            self.candidate = None;
            if self.last_saved_at.is_some_and(|saved| saved > now) {
                self.last_saved_at = Some(now);
            }
        }
        self.last_observed_at = Some(now);
        let changed_candidate = self.candidate.as_ref().is_none_or(|candidate| {
            fingerprint.difference(&candidate.fingerprint) >= self.config.threshold
        });
        if changed_candidate {
            self.candidate = Some(Candidate {
                fingerprint: fingerprint.clone(),
                since: now,
            });
        }
        let Some(candidate) = &self.candidate else {
            return false;
        };
        if now.saturating_sub(candidate.since) < self.config.settle {
            return false;
        }
        if self
            .last_saved_at
            .is_some_and(|saved| now.saturating_sub(saved) < self.config.cooldown)
        {
            return false;
        }
        self.last_saved
            .as_ref()
            .is_none_or(|saved| fingerprint.difference(saved) >= self.config.threshold)
    }

    /// Acknowledge the actual image saved to disk, not merely a candidate frame.
    pub fn mark_saved(&mut self, image: &RgbaImage, now: Duration) {
        if check_image(image).is_err() {
            return;
        }
        let fingerprint = Fingerprint::from_image(image);
        self.candidate = Some(Candidate {
            fingerprint: fingerprint.clone(),
            since: now,
        });
        self.last_saved = Some(fingerprint);
        self.last_saved_at = Some(now);
        self.last_observed_at = Some(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn frame(intensity: u8) -> RgbaImage {
        RgbaImage::from_pixel(128, 72, Rgba([intensity, intensity, intensity, 255]))
    }

    fn at(milliseconds: u64) -> Duration {
        Duration::from_millis(milliseconds)
    }

    #[test]
    fn utc_timestamps_cover_epoch_leap_day_and_day_boundary() {
        assert_eq!(timestamp_utc(0), "19700101-000000");
        assert_eq!(timestamp_utc(86_399), "19700101-235959");
        assert_eq!(timestamp_utc(86_400), "19700102-000000");
        assert_eq!(timestamp_utc(951_827_696), "20000229-123456");
    }

    #[test]
    fn titles_cannot_escape_the_capture_directory() {
        assert_eq!(
            slug("../../Secret\\client:  Quarterly / Review.png"),
            "secret-client-quarterly-review-png"
        );
        assert_eq!(slug("../\\ \n"), "capture");
        assert!(slug(&"界".repeat(80)).len() <= 56);
        assert_eq!(display_title("  Board\n  meeting\t\0"), "Board meeting");
    }

    #[test]
    fn sessions_do_not_reuse_existing_directories() {
        let base = tempfile::tempdir().unwrap();
        let first = SessionStore::create(base.path(), "Daily work").unwrap();
        let second = SessionStore::create(base.path(), "Daily work").unwrap();
        assert_ne!(first.folder(), second.folder());
        assert!(first.folder().is_dir());
        assert!(second.folder().is_dir());
    }

    #[test]
    fn saves_are_pngs_with_sequential_names_and_no_sidecars() {
        let base = tempfile::tempdir().unwrap();
        let mut store = SessionStore::create(base.path(), "Work").unwrap();
        let pixels = frame(90);
        let first = store
            .save(&pixels, Some("../Board / slides"), Trigger::Manual)
            .unwrap();
        let second = store.save(&pixels, None, Trigger::Interval).unwrap();
        assert_eq!(first.sequence, 1);
        assert_eq!(second.sequence, 2);
        assert_eq!(first.path.parent(), Some(store.folder()));
        assert!(first
            .path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .ends_with("-0001-board-slides.png"));
        assert_eq!(image::open(&first.path).unwrap().into_rgba8(), pixels);
        assert_eq!(fs::read_dir(store.folder()).unwrap().count(), 2);
    }

    #[test]
    fn save_failure_keeps_prior_files_and_sequence() {
        let base = tempfile::tempdir().unwrap();
        let mut store = SessionStore::create(base.path(), "Work").unwrap();
        let first = store.save(&frame(60), None, Trigger::Manual).unwrap();
        let original = fs::read(&first.path).unwrap();
        assert!(store
            .save(&RgbaImage::new(0, 1), None, Trigger::Manual)
            .is_err());
        assert_eq!(store.sequence, 1);
        assert_eq!(fs::read(&first.path).unwrap(), original);
        assert_eq!(fs::read_dir(store.folder()).unwrap().count(), 1);
    }

    #[test]
    fn preexisting_capture_name_is_not_overwritten() {
        let base = tempfile::tempdir().unwrap();
        let mut store = SessionStore::create(base.path(), "Work").unwrap();
        // Occupy this and the next UTC second, avoiding a test boundary race.
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let occupied: Vec<_> = (seconds..=seconds + 2)
            .map(|now| {
                let path = store
                    .folder()
                    .join(format!("{}-0001-capture.png", timestamp_utc(now)));
                fs::write(&path, b"existing capture").unwrap();
                path
            })
            .collect();
        let result = store
            .save(&frame(90), Some("Capture"), Trigger::Manual)
            .unwrap();
        assert_eq!(result.sequence, 2);
        for path in occupied {
            assert_eq!(fs::read(path).unwrap(), b"existing capture");
        }
    }

    #[test]
    fn rename_preserves_bytes_prefix_and_existing_names() {
        let base = tempfile::tempdir().unwrap();
        let mut store = SessionStore::create(base.path(), "Work").unwrap();
        let capture = store
            .save(&frame(100), Some("Original"), Trigger::Manual)
            .unwrap();
        let bytes = fs::read(&capture.path).unwrap();
        let filename = capture.path.file_name().unwrap().to_str().unwrap();
        let occupied = store
            .folder()
            .join(filename.replace("original", "board-slides"));
        fs::write(&occupied, b"untouched").unwrap();
        let renamed = rename_capture(&capture, "../../Board slides").unwrap();
        assert_eq!(renamed.sequence, capture.sequence);
        assert_eq!(renamed.path.parent(), capture.path.parent());
        assert!(!capture.path.exists());
        assert!(renamed
            .path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .ends_with("-0001-board-slides-02.png"));
        assert_eq!(fs::read(&renamed.path).unwrap(), bytes);
        assert_eq!(fs::read(&occupied).unwrap(), b"untouched");
        assert_eq!(rename_capture(&renamed, "  ").unwrap().path, renamed.path);
    }

    #[test]
    fn handle_manual_rename_wins_over_pending_ocr() {
        let base = tempfile::tempdir().unwrap();
        let mut store = SessionStore::create(base.path(), "Work").unwrap();
        let pixels = frame(100);
        let original = store.save(&pixels, None, Trigger::Manual).unwrap();
        let handle = CaptureHandle::new(original.clone());
        let ocr_handle = handle.clone();

        let manual = handle.rename("My chosen title").unwrap();
        assert!(ocr_handle
            .rename_if_unchanged(&original, "Automatic title")
            .unwrap()
            .is_none());
        let current = ocr_handle.snapshot().unwrap();
        assert_eq!(current.title, "My chosen title");
        assert_eq!(current.path, manual.path);
        assert_eq!(ocr_handle.load().unwrap(), (pixels, manual.path));
        assert_eq!(fs::read_dir(store.folder()).unwrap().count(), 1);
    }

    #[test]
    fn handle_manual_name_survives_an_older_ocr_notification() {
        let base = tempfile::tempdir().unwrap();
        let mut store = SessionStore::create(base.path(), "Work").unwrap();
        let original = store.save(&frame(90), None, Trigger::Manual).unwrap();
        let handle = CaptureHandle::new(original.clone());
        let ui_handle = handle.clone();
        // OCR finishes, but its notification has not yet reached the UI.
        let stale_notification = handle
            .rename_if_unchanged(&original, "OCR title")
            .unwrap()
            .unwrap();
        let manual = ui_handle.rename("User title").unwrap();

        assert_ne!(stale_notification.path, manual.path);
        assert!(!stale_notification.path.exists());
        // Refreshing from the shared handle cannot restore the stale event.
        assert_eq!(ui_handle.snapshot().unwrap().title, "User title");
        assert_eq!(handle.snapshot().unwrap().path, manual.path);
        assert!(handle
            .rename_if_unchanged(&stale_notification, "Another OCR title")
            .unwrap()
            .is_none());
        assert_eq!(fs::read_dir(store.folder()).unwrap().count(), 1);
    }

    #[test]
    fn handle_explicitly_keeping_the_same_name_blocks_ocr() {
        let base = tempfile::tempdir().unwrap();
        let mut store = SessionStore::create(base.path(), "Work").unwrap();
        let original = store.save(&frame(90), None, Trigger::Manual).unwrap();
        let handle = CaptureHandle::new(original.clone());

        handle.rename(&original.title).unwrap();
        assert!(handle
            .rename_if_unchanged(&original, "OCR title")
            .unwrap()
            .is_none());
        assert_eq!(handle.snapshot().unwrap().path, original.path);
        assert_eq!(handle.snapshot().unwrap().title, original.title);
    }

    #[test]
    fn handle_rejects_an_ocr_result_for_an_outdated_path() {
        let base = tempfile::tempdir().unwrap();
        let mut store = SessionStore::create(base.path(), "Work").unwrap();
        let original = store.save(&frame(90), None, Trigger::Manual).unwrap();
        let handle = CaptureHandle::new(original.clone());
        let named = handle
            .rename_if_unchanged(&original, "First OCR result")
            .unwrap()
            .unwrap();

        assert!(handle
            .rename_if_unchanged(&original, "Stale OCR result")
            .unwrap()
            .is_none());
        assert_eq!(handle.snapshot().unwrap().path, named.path);
    }

    #[test]
    fn handle_load_serializes_with_rename_without_blocking_other_captures() {
        use std::sync::mpsc;

        let base = tempfile::tempdir().unwrap();
        let mut store = SessionStore::create(base.path(), "Work").unwrap();
        let pixels = frame(70);
        let original = store.save(&pixels, None, Trigger::Manual).unwrap();
        let handle = CaptureHandle::new(original.clone());
        let other = CaptureHandle::new(store.save(&frame(90), None, Trigger::Manual).unwrap());
        let (entered, entered_rx) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let reader_handle = handle.clone();
        let reader = std::thread::spawn(move || {
            reader_handle.load_with(|path| {
                entered.send(path.to_path_buf()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                crate::editor::load_image(path)
            })
        });
        let reading_path = entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(reading_path, original.path);
        assert!(handle.inner.try_lock().is_err());

        let (attempted, attempted_rx) = mpsc::channel();
        let (renamed, renamed_rx) = mpsc::channel();
        let rename_handle = handle.clone();
        let renamer = std::thread::spawn(move || {
            attempted.send(()).unwrap();
            let result = rename_handle.rename_if_unchanged(&original, "OCR title");
            renamed.send(result).unwrap();
        });
        attempted_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(
            renamed_rx.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        assert!(reading_path.exists());
        // Another capture has its own lock and remains usable during decoding.
        assert_eq!(
            other.rename("Independent name").unwrap().title,
            "Independent name"
        );

        release.send(()).unwrap();
        let loaded = reader.join().unwrap().unwrap();
        let renamed = renamed_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap()
            .unwrap();
        renamer.join().unwrap();
        assert_eq!(loaded, (pixels.clone(), reading_path.clone()));
        assert!(!reading_path.exists());
        assert_eq!(handle.load().unwrap(), (pixels, renamed.path));
    }

    #[test]
    fn first_frame_requires_settling_and_save_acknowledgement() {
        let mut detector = ChangeDetector::new(ChangeConfig::default());
        let image = frame(20);
        assert!(!detector.observe(&image, at(0)));
        assert!(!detector.observe(&image, at(799)));
        assert!(detector.observe(&image, at(800)));
        assert!(
            detector.observe(&image, at(900)),
            "a failed save must remain retryable"
        );
        detector.mark_saved(&image, at(900));
        assert!(!detector.observe(&image, at(5000)));
        assert!(!detector.observe(&image, at(9000)));
    }

    #[test]
    fn transition_refreshes_settling_and_then_saves_once() {
        let mut detector = ChangeDetector::new(ChangeConfig::default());
        detector.mark_saved(&frame(0), at(0));
        assert!(!detector.observe(&frame(80), at(2200)));
        assert!(!detector.observe(&frame(160), at(2700)));
        assert!(!detector.observe(&frame(160), at(3499)));
        assert!(detector.observe(&frame(160), at(3500)));
        detector.mark_saved(&frame(160), at(3500));
        assert!(!detector.observe(&frame(160), at(8000)));
    }

    #[test]
    fn cooldown_delays_a_settled_change() {
        let mut detector = ChangeDetector::new(ChangeConfig::default());
        detector.mark_saved(&frame(0), at(0));
        assert!(!detector.observe(&frame(200), at(100)));
        assert!(!detector.observe(&frame(200), at(1000)));
        assert!(!detector.observe(&frame(200), at(1999)));
        assert!(detector.observe(&frame(200), at(2000)));
    }

    #[test]
    fn tiny_cursor_and_small_animation_do_not_trigger_or_block_real_change() {
        let mut detector = ChangeDetector::new(ChangeConfig::default());
        let blank = frame(0);
        detector.mark_saved(&blank, at(0));
        let mut cursor = blank.clone();
        for y in 0..8 {
            for x in 0..8 {
                cursor.put_pixel(x, y, Rgba([255; 4]));
            }
        }
        assert!(!detector.observe(&cursor, at(3000)));
        assert!(!detector.observe(&blank, at(4000)));
        let changed = frame(100);
        assert!(!detector.observe(&changed, at(5000)));
        let mut with_animation = changed.clone();
        for y in 0..10 {
            for x in 0..20 {
                with_animation.put_pixel(x, y, Rgba([255; 4]));
            }
        }
        assert!(detector.observe(&with_animation, at(5800)));
    }

    #[test]
    fn low_intensity_noise_and_return_to_baseline_are_ignored() {
        let mut detector = ChangeDetector::new(ChangeConfig::default());
        detector.mark_saved(&frame(100), at(0));
        assert!(!detector.observe(&frame(115), at(4000)));
        assert!(!detector.observe(&frame(200), at(5000)));
        assert!(!detector.observe(&frame(100), at(5400)));
        assert!(!detector.observe(&frame(100), at(7000)));
    }

    #[test]
    fn invalid_config_is_bounded_and_backwards_clock_recovers() {
        let config = ChangeConfig {
            threshold: f32::NAN,
            ..ChangeConfig::default()
        };
        let mut detector = ChangeDetector::new(config);
        assert_eq!(detector.config.threshold, 0.04);
        assert_eq!(
            ChangeDetector::new(ChangeConfig {
                threshold: -1.0,
                ..ChangeConfig::default()
            })
            .config
            .threshold,
            0.005
        );
        detector.mark_saved(&frame(0), at(10_000));
        assert!(!detector.observe(&frame(200), at(100)));
        assert!(detector.observe(&frame(200), at(2100)));
        assert!(!detector.observe(&RgbaImage::new(0, 0), at(2200)));
    }
}
