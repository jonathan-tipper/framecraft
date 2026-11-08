//! First-run preferences and collision-safe output for one-off screenshots.

use crate::settings::{canonical_key, display_shortcut, ShortcutSettings};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum AfterCapture {
    #[default]
    Edit,
    Save,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct QuickCaptureSettings {
    pub setup_complete: bool,
    pub shortcut_enabled: bool,
    pub shortcut: String,
    pub destination: PathBuf,
    pub filename_prefix: String,
    pub organise_by_date: bool,
    pub after_capture: AfterCapture,
}
impl Default for QuickCaptureSettings {
    fn default() -> Self {
        Self {
            setup_complete: false,
            shortcut_enabled: true,
            shortcut: "Command + Shift + 2".into(),
            destination: std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join("Pictures/Framecraft"),
            filename_prefix: "Screenshot".into(),
            organise_by_date: false,
            after_capture: AfterCapture::Edit,
        }
    }
}
impl QuickCaptureSettings {
    pub fn validate(&self, session: &ShortcutSettings) -> Result<(), String> {
        self.validate_output()?;
        let key = canonical_quick_shortcut(&self.shortcut)?;
        if self.shortcut_enabled && session.canonical_keys()?.contains(&key) {
            return Err("The capture shortcut is already used by a session command. Choose another combination.".into());
        }
        Ok(())
    }
    fn validate_output(&self) -> Result<(), String> {
        if !self.destination.is_absolute()
            || self
                .destination
                .components()
                .any(|p| matches!(p, std::path::Component::ParentDir))
        {
            return Err(
                "Choose an absolute destination folder without parent-path components.".into(),
            );
        }
        let prefix = &self.filename_prefix;
        if prefix.is_empty()
            || prefix.chars().count() > 64
            || prefix.trim() != prefix
            || prefix.starts_with('.')
            || prefix.ends_with('.')
            || prefix
                .chars()
                .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
            || prefix == "."
            || prefix == ".."
        {
            return Err("Use a filename prefix of 1–64 characters without slashes, colons, control characters or leading/trailing spaces or dots.".into());
        }
        Ok(())
    }
    pub fn path() -> Result<PathBuf, String> {
        let home = std::env::var_os("HOME").ok_or("Could not locate your settings folder.")?;
        Ok(PathBuf::from(home).join("Library/Application Support/Framecraft/quick-capture.json"))
    }
    pub fn load(session: &ShortcutSettings) -> Result<Self, String> {
        Self::load_from(&Self::path()?, session)
    }
    pub fn load_from(path: &Path, session: &ShortcutSettings) -> Result<Self, String> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(format!("Could not read capture settings: {e}")),
        };
        if bytes.len() > 16384 {
            return Err(
                "Capture settings are too large. Review setup before saving new settings.".into(),
            );
        }
        let mut value: Self = serde_json::from_slice(&bytes)
            .map_err(|e| format!("Could not read capture settings: {e}"))?;
        value.validate(session)?;
        value.shortcut = display_shortcut(&value.shortcut)?;
        Ok(value)
    }
    pub fn save(&self, session: &ShortcutSettings) -> Result<(), String> {
        self.save_to(&Self::path()?, session)
    }
    pub fn save_to(&self, path: &Path, session: &ShortcutSettings) -> Result<(), String> {
        self.validate(session)?;
        let parent = path.parent().ok_or("Settings path has no parent folder.")?;
        fs::create_dir_all(parent).map_err(|e| format!("Could not create settings folder: {e}"))?;
        let mut stage = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        stage
            .write_all(&serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        stage.as_file().sync_all().map_err(|e| e.to_string())?;
        stage
            .persist(path)
            .map_err(|e| format!("Could not save capture settings: {e}"))?;
        Ok(())
    }
}

/// Validation shared by preferences and the native registration boundary.
pub(crate) fn canonical_quick_shortcut(value: &str) -> Result<String, String> {
    let key = canonical_key(value)?;
    if matches!(
        key.as_str(),
        "Super+KeyQ"
            | "Super+KeyO"
            | "Super+KeyS"
            | "Super+KeyZ"
            | "Super+Shift+KeyZ"
            | "Super+Shift+KeyC"
            | "Super+KeyH"
            | "Super+Alt+KeyH"
            | "Super+Alt+Digit3"
            | "Super+Alt+Digit4"
            | "Super+Alt+Digit5"
            | "Super+Shift+Digit3"
            | "Super+Shift+Digit4"
            | "Super+Shift+Digit5"
    ) {
        return Err(format!(
            "{} is reserved by Framecraft or macOS. Choose another shortcut.",
            display_shortcut(value)?
        ));
    }
    Ok(key)
}

/// The caller retains the captured pixels if saving fails. This never renders
/// editor framing or overwrites an existing screenshot. Call on a worker.
pub fn save_capture(image: &RgbaImage, settings: &QuickCaptureSettings) -> Result<PathBuf, String> {
    save_capture_cancellable(image, settings, &|| false)?
        .ok_or_else(|| "Capture was cancelled.".into())
}

/// Cancellation before the publication boundary removes the private staging
/// file and returns None. Once atomic publication begins, saving completes;
/// cancellation cannot retract a screenshot that has already been published.
pub fn save_capture_cancellable(
    image: &RgbaImage,
    settings: &QuickCaptureSettings,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<PathBuf>, String> {
    if cancelled() {
        return Ok(None);
    }
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "Correct the system clock before saving.")?
        .as_secs();
    save_capture_at_cancellable(image, settings, &timestamp_utc(seconds), cancelled)
}

/// UTC stamp is YYYYMMDD-HHMMSS. Injected for deterministic naming tests.
pub fn save_capture_at(
    image: &RgbaImage,
    settings: &QuickCaptureSettings,
    stamp: &str,
) -> Result<PathBuf, String> {
    save_capture_at_cancellable(image, settings, stamp, &|| false)?
        .ok_or_else(|| "Capture was cancelled.".into())
}

fn save_capture_at_cancellable(
    image: &RgbaImage,
    settings: &QuickCaptureSettings,
    stamp: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<PathBuf>, String> {
    settings.validate_output()?;
    validate_stamp(stamp)?;
    if image.width() == 0
        || image.height() == 0
        || image.width() > crate::editor::MAX_DIMENSION
        || image.height() > crate::editor::MAX_DIMENSION
        || u64::from(image.width()) * u64::from(image.height()) > crate::editor::MAX_PIXELS
    {
        return Err("Image exceeds the supported pixel bounds.".into());
    }
    if !settings.destination.is_dir() {
        return Err("The save folder is unavailable. Choose a folder in Capture Setup; the screenshot can still be edited or saved manually.".into());
    }
    let destination = if settings.organise_by_date {
        let folder =
            settings
                .destination
                .join(format!("{}-{}-{}", &stamp[..4], &stamp[4..6], &stamp[6..8]));
        fs::create_dir_all(&folder).map_err(|e| format!("Could not create date folder: {e}"))?;
        folder
    } else {
        settings.destination.clone()
    };
    let mut stage = tempfile::Builder::new()
        .prefix(".framecraft-capture-")
        .tempfile_in(&destination)
        .map_err(|e| format!("Could not save screenshot: {e}"))?;
    image
        .write_to(stage.as_file_mut(), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    stage.as_file().sync_all().map_err(|e| e.to_string())?;
    for sequence in 1..=9999 {
        if cancelled() {
            return Ok(None);
        }
        let suffix = if sequence == 1 {
            String::new()
        } else {
            format!("-{sequence:04}")
        };
        let path = destination.join(format!("{}-{stamp}{suffix}.png", settings.filename_prefix));
        match stage.persist_noclobber(&path) {
            Ok(_) => {
                fs::File::open(&destination).and_then(|f| f.sync_all()).map_err(|e| format!("Screenshot saved to {} but directory durability could not be confirmed: {e}. Inspect the file before retrying.", path.display()))?;
                return Ok(Some(path));
            }
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                stage = error.file;
            }
            Err(error) => return Err(format!("Could not save screenshot: {}", error.error)),
        }
    }
    Err("Too many screenshots share this filename. Choose a different prefix.".into())
}

fn validate_stamp(stamp: &str) -> Result<(), String> {
    let error = || "Capture timestamp is invalid.".to_string();
    if stamp.len() != 15
        || stamp.as_bytes()[8] != b'-'
        || !stamp
            .bytes()
            .enumerate()
            .all(|(i, b)| i == 8 || b.is_ascii_digit())
    {
        return Err(error());
    }
    let number = |a, b| stamp[a..b].parse::<u32>().unwrap();
    let (year, month, day) = (number(0, 4), number(4, 6), number(6, 8));
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    if year < 1970
        || day == 0
        || day > days
        || number(9, 11) > 23
        || number(11, 13) > 59
        || number(13, 15) > 59
    {
        return Err(error());
    }
    Ok(())
}

// Gregorian UTC conversion, matching existing session filenames.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(root: &Path) -> QuickCaptureSettings {
        QuickCaptureSettings {
            destination: root.into(),
            ..Default::default()
        }
    }
    #[test]
    fn settings_round_trip_without_touching_session_preferences() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("quick.json");
        let mut value = settings(directory.path());
        value.setup_complete = true;
        value.after_capture = AfterCapture::Save;
        value.save_to(&path, &ShortcutSettings::default()).unwrap();
        assert_eq!(
            QuickCaptureSettings::load_from(&path, &ShortcutSettings::default()).unwrap(),
            value
        );
        assert!(!directory.path().join("shortcuts.json").exists());
    }
    #[test]
    fn unsafe_names_relative_destinations_and_shortcut_conflicts_fail() {
        let directory = tempfile::tempdir().unwrap();
        for prefix in [
            "",
            "..",
            "a/b",
            "a\\b",
            " bad",
            "bad.",
            "bad\nname",
            "a:b",
            ".hidden",
        ] {
            let mut value = settings(directory.path());
            value.filename_prefix = prefix.into();
            assert!(
                value.validate(&ShortcutSettings::default()).is_err(),
                "{prefix}"
            );
        }
        let mut value = settings(Path::new("relative"));
        assert!(value.validate(&ShortcutSettings::default()).is_err());
        value.destination = directory.path().into();
        for key in [
            "Command + Q",
            "Command + Shift + 4",
            "Command + Option + S",
            "S",
        ] {
            value.shortcut = key.into();
            assert!(
                value.validate(&ShortcutSettings::default()).is_err(),
                "{key}"
            );
        }
    }
    #[test]
    fn malformed_settings_are_reported_without_overwriting_them() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("quick.json");
        std::fs::write(&path, b"broken").unwrap();
        assert!(QuickCaptureSettings::load_from(&path, &ShortcutSettings::default()).is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"broken");
    }
    #[test]
    fn captures_keep_original_pixels_and_never_overwrite_collisions() {
        let directory = tempfile::tempdir().unwrap();
        let mut value = settings(directory.path());
        value.organise_by_date = true;
        let image = RgbaImage::from_pixel(8, 6, image::Rgba([12, 44, 78, 255]));
        let a = save_capture_at(&image, &value, "20261108-142233").unwrap();
        let b = save_capture_at(&image, &value, "20261108-142233").unwrap();
        assert_ne!(a, b);
        assert_eq!(a.parent().unwrap().file_name().unwrap(), "2026-11-08");
        assert_eq!(image::open(&a).unwrap().to_rgba8(), image);
        assert_eq!(image::open(&b).unwrap().to_rgba8(), image);
        std::fs::write(&a, b"preserve").unwrap();
        save_capture_at(&image, &value, "20261108-142233").unwrap();
        assert_eq!(std::fs::read(a).unwrap(), b"preserve");
    }
    #[test]
    fn invalid_stamp_missing_folder_and_invalid_image_do_not_publish() {
        let directory = tempfile::tempdir().unwrap();
        let value = settings(directory.path());
        let image = RgbaImage::new(1, 1);
        for stamp in [
            "../../escape",
            "20261108-142233/x",
            "20261108-252233",
            "20260230-120000",
        ] {
            assert!(save_capture_at(&image, &value, stamp).is_err());
        }
        assert!(save_capture_at(&RgbaImage::new(0, 0), &value, "20261108-142233").is_err());
        let missing = settings(&directory.path().join("missing"));
        assert!(save_capture_at(&image, &missing, "20261108-142233").is_err());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }
    #[test]
    fn concurrent_captures_reserve_distinct_complete_files() {
        let directory = tempfile::tempdir().unwrap();
        let value = settings(directory.path());
        let workers: Vec<_> = (1..=4)
            .map(|n| {
                let settings = value.clone();
                std::thread::spawn(move || {
                    let image = RgbaImage::from_pixel(4, 4, image::Rgba([n, 20, 30, 255]));
                    let path = save_capture_at(&image, &settings, "20261108-142233").unwrap();
                    assert_eq!(image::open(&path).unwrap().to_rgba8(), image);
                    path
                })
            })
            .collect();
        let paths: std::collections::HashSet<_> =
            workers.into_iter().map(|w| w.join().unwrap()).collect();
        assert_eq!(paths.len(), 4);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 4);
    }
    #[test]
    fn invalid_new_preferences_leave_the_saved_preferences_unchanged() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("quick.json");
        let mut value = settings(directory.path());
        value.save_to(&path, &ShortcutSettings::default()).unwrap();
        let old = std::fs::read(&path).unwrap();
        value.filename_prefix = "../bad".into();
        assert!(value.save_to(&path, &ShortcutSettings::default()).is_err());
        assert_eq!(std::fs::read(path).unwrap(), old);
    }
    #[test]
    fn cancellation_during_staging_leaves_no_png_or_temporary_file() {
        let directory = tempfile::tempdir().unwrap();
        let value = settings(directory.path());
        let checks = std::cell::Cell::new(0);
        let result = save_capture_cancellable(&RgbaImage::new(32, 32), &value, &|| {
            checks.set(checks.get() + 1);
            if checks.get() == 2 {
                let staged = std::fs::read_dir(directory.path())
                    .unwrap()
                    .next()
                    .unwrap()
                    .unwrap();
                assert!(staged
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".framecraft-capture-"));
                assert!(staged.metadata().unwrap().len() > 0);
            }
            checks.get() >= 2
        })
        .unwrap();
        assert!(result.is_none());
        assert_eq!(checks.get(), 2);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }
    #[test]
    fn cancellation_after_a_collision_preserves_existing_file_and_removes_stage() {
        let directory = tempfile::tempdir().unwrap();
        let value = settings(directory.path());
        let path = directory.path().join("Screenshot-20261108-142233.png");
        std::fs::write(&path, b"existing").unwrap();
        let checks = std::cell::Cell::new(0);
        let result =
            save_capture_at_cancellable(&RgbaImage::new(4, 4), &value, "20261108-142233", &|| {
                checks.set(checks.get() + 1);
                checks.get() == 2
            })
            .unwrap();
        assert!(result.is_none());
        assert_eq!(checks.get(), 2);
        assert_eq!(std::fs::read(&path).unwrap(), b"existing");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }
    #[test]
    fn cancellation_before_work_does_not_create_the_destination() {
        let directory = tempfile::tempdir().unwrap();
        let value = settings(&directory.path().join("missing"));
        assert!(
            save_capture_cancellable(&RgbaImage::new(1, 1), &value, &|| true)
                .unwrap()
                .is_none()
        );
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }
    #[test]
    fn utc_timestamp_covers_epoch_and_leap_day() {
        assert_eq!(timestamp_utc(0), "19700101-000000");
        assert_eq!(timestamp_utc(1709164800), "20240229-000000");
    }
}
