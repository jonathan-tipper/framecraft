//! macOS capture adapter for the first screenshot-only release.
//!
//! The editor and application are Rust. This adapter deliberately delegates
//! selection and pixel capture to macOS's `screencapture` executable; it is not
//! a direct ScreenCaptureKit implementation. A future adapter can use
//! `objc2-screen-capture-kit` without changing the editor's image contract.

use image::RgbaImage;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureMode {
    Area,
    Window,
    Screen,
}

/// Capture a screenshot, returning `None` when the native selector is cancelled.
///
/// Call on a worker thread after hiding the editor. Native selection can remain
/// open until the user completes it, so this must not block the UI event loop.
pub fn capture(mode: CaptureMode) -> Result<Option<RgbaImage>, String> {
    #[cfg(target_os = "macos")]
    {
        capture_macos(mode)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = mode;
        Err("Screen capture is available on macOS. You can still open and edit a local image on this platform.".into())
    }
}

#[cfg(any(target_os = "macos", test))]
fn command_args(mode: CaptureMode) -> Vec<&'static str> {
    let mut args = vec!["-x", "-t", "png"];
    match mode {
        CaptureMode::Area => args.extend(["-i", "-s"]),
        CaptureMode::Window => args.extend(["-i", "-w"]),
        // Without -m, screencapture can create a separate file per display.
        CaptureMode::Screen => args.push("-m"),
    }
    args
}

#[cfg(target_os = "macos")]
fn capture_macos(mode: CaptureMode) -> Result<Option<RgbaImage>, String> {
    run_capture(mode, &command_args(mode))
}

/// Snapshot one display without opening a picker. The index is macOS's -D index.
pub fn capture_display(display: u32) -> Result<RgbaImage, String> {
    if !(1..=16).contains(&display) {
        return Err("Choose a display number from 1 to 16.".into());
    }
    #[cfg(target_os = "macos")]
    {
        let index = display.to_string();
        run_capture(CaptureMode::Screen, &["-x", "-t", "png", "-D", &index])?
            .ok_or_else(|| "The display did not produce a screenshot.".into())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("Display capture requires macOS.".into())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CaptureTarget {
    pub display: u32,
    pub region: crate::editor::Rect,
    pub source_width: u32,
    pub source_height: u32,
}

impl CaptureTarget {
    pub fn capture(&self) -> Result<RgbaImage, String> {
        self.crop(&capture_display(self.display)?)
    }

    pub fn crop(&self, image: &RgbaImage) -> Result<RgbaImage, String> {
        if image.dimensions() != (self.source_width, self.source_height) {
            return Err(
                "Display size or scale changed. Stop this session and select the area again."
                    .into(),
            );
        }
        let r = self.region;
        if r.width == 0
            || r.height == 0
            || u64::from(r.x) + u64::from(r.width) > u64::from(image.width())
            || u64::from(r.y) + u64::from(r.height) > u64::from(image.height())
        {
            return Err("The selected area is outside the display. Select it again.".into());
        }
        Ok(image::imageops::crop_imm(image, r.x, r.y, r.width, r.height).to_image())
    }
}

#[cfg(target_os = "macos")]
fn run_capture(mode: CaptureMode, args: &[&str]) -> Result<Option<RgbaImage>, String> {
    use std::fs;
    use std::io::ErrorKind;
    use std::os::unix::fs::PermissionsExt;
    use std::process::{Command, Stdio};

    // tempfile creates a private directory; explicitly preserve its 0700 mode.
    // RAII removes both the capture and the directory on success, cancellation,
    // decoder failure, and command failure. The editor receives only pixels.
    let temporary = tempfile::Builder::new()
        .prefix("framecraft-capture-")
        .tempdir()
        .map_err(|e| format!("Could not create private capture storage: {e}"))?;
    fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("Could not protect private capture storage: {e}"))?;
    let path = temporary.path().join("capture.png");

    let output = Command::new("/usr/sbin/screencapture")
        .args(args)
        .arg(&path)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| {
            format!("Could not start macOS screen capture: {e}. Check that /usr/sbin/screencapture is available.")
        })?;

    let file_exists = match fs::metadata(&path) {
        Ok(metadata) => metadata.is_file(),
        Err(error) if error.kind() == ErrorKind::NotFound => false,
        Err(error) => return Err(format!("Could not inspect the captured image: {error}")),
    };
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !classify_completion(mode, output.status.success(), file_exists, &stderr)? {
        return Ok(None);
    }

    // The shared decoder checks dimensions before allocating the full image,
    // applying the same 64-megapixel ceiling as Open Image.
    crate::editor::load_image(&path)
        .map(Some)
        .map_err(|e| format!("macOS captured an image, but Framecraft could not open it: {e}"))
}

#[cfg(any(target_os = "macos", test))]
fn classify_completion(
    mode: CaptureMode,
    success: bool,
    file_exists: bool,
    stderr: &str,
) -> Result<bool, String> {
    let diagnostic = stderr.trim();
    let interactive = mode != CaptureMode::Screen;
    if success && file_exists {
        return Ok(true);
    }

    // screencapture has no documented, unique cancellation exit code. Do not
    // turn arbitrary nonzero exits or missing files into silent cancellation.
    // Recognise a clean interactive completion without output, or an explicit
    // cancellation message, and surface all ambiguous failures to the user.
    let explicit_cancellation = matches!(
        diagnostic.to_ascii_lowercase().as_str(),
        "cancelled"
            | "canceled"
            | "user cancelled"
            | "user canceled"
            | "screencapture: cancelled"
            | "screencapture: canceled"
            | "screencapture: user cancelled"
            | "screencapture: user canceled"
    );
    if interactive && !file_exists && ((success && diagnostic.is_empty()) || explicit_cancellation)
    {
        return Ok(false);
    }

    let detail = if diagnostic.is_empty() {
        if success {
            "The capture command finished without creating an image.".to_owned()
        } else {
            "The capture command did not complete successfully. If you pressed Escape, no image was saved.".to_owned()
        }
    } else {
        diagnostic.chars().take(600).collect()
    };
    Err(format!(
        "Screen capture failed. {detail} If macOS blocked access, open System Settings > Privacy & Security > Screen & System Audio Recording (Screen Recording on older macOS), allow Framecraft, then quit and reopen it. When using cargo run, macOS may list Terminal or your IDE instead."
    ))
}

#[cfg(test)]
mod tests {
    use super::{classify_completion, command_args, CaptureMode};

    #[test]
    fn repeated_region_crops_in_physical_pixels_and_rejects_size_changes() {
        let image =
            image::RgbaImage::from_fn(200, 100, |x, y| image::Rgba([x as u8, y as u8, 0, 255]));
        let target = super::CaptureTarget {
            display: 1,
            region: crate::editor::Rect {
                x: 150,
                y: 70,
                width: 50,
                height: 30,
            },
            source_width: 200,
            source_height: 100,
        };
        let cropped = target.crop(&image).unwrap();
        assert_eq!(cropped.dimensions(), (50, 30));
        assert_eq!(cropped.get_pixel(49, 29).0, [199, 99, 0, 255]);
        assert!(target.crop(&image::RgbaImage::new(400, 200)).is_err());
    }

    #[test]
    fn area_uses_interactive_selection_and_png() {
        assert_eq!(
            command_args(CaptureMode::Area),
            ["-x", "-t", "png", "-i", "-s"]
        );
    }

    #[test]
    fn window_uses_interactive_window_capture() {
        assert_eq!(
            command_args(CaptureMode::Window),
            ["-x", "-t", "png", "-i", "-w"]
        );
    }

    #[test]
    fn screen_only_captures_primary_display() {
        assert_eq!(command_args(CaptureMode::Screen), ["-x", "-t", "png", "-m"]);
    }

    #[test]
    fn clean_interactive_exit_without_file_is_cancelled() {
        assert_eq!(
            classify_completion(CaptureMode::Area, true, false, ""),
            Ok(false)
        );
    }

    #[test]
    fn known_cancellation_message_is_cancelled() {
        assert_eq!(
            classify_completion(
                CaptureMode::Window,
                false,
                false,
                "screencapture: cancelled\n"
            ),
            Ok(false)
        );
    }

    #[test]
    fn permission_error_is_not_silently_cancelled() {
        assert!(classify_completion(
            CaptureMode::Area,
            false,
            false,
            "could not create image from display"
        )
        .is_err());
        assert!(classify_completion(CaptureMode::Area, false, false, "").is_err());
    }

    #[test]
    fn noninteractive_missing_output_is_error() {
        assert!(classify_completion(CaptureMode::Screen, true, false, "").is_err());
    }

    #[test]
    fn failed_process_with_partial_file_is_error() {
        assert!(classify_completion(CaptureMode::Area, false, true, "capture failed").is_err());
    }
}
