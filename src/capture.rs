//! macOS capture adapter for the first screenshot-only release.
//!
//! The editor and application are Rust. This adapter deliberately delegates
//! selection and pixel capture to macOS's `screencapture` executable; it is not
//! a direct ScreenCaptureKit implementation. A future adapter can use
//! `objc2-screen-capture-kit` without changing the editor's image contract.

use image::RgbaImage;
use std::{path::Path, time::Duration};

/// Interactive selection remains user-controlled. Repeated unattended captures
/// must finish or be terminated before another sample can start.
pub const CAPTURE_DEADLINE: Duration = Duration::from_secs(10);

type VisibilityHandler = std::sync::Arc<dyn Fn(bool) -> Result<(), String> + Send + Sync>;
static VISIBILITY_HANDLER: std::sync::Mutex<Option<VisibilityHandler>> =
    std::sync::Mutex::new(None);

/// Desktop UI registers an acknowledged, main-thread hide/show operation.
/// The worker fails closed if controls cannot be hidden before capture.
pub fn set_visibility_handler(handler: Option<VisibilityHandler>) {
    if let Ok(mut current) = VISIBILITY_HANDLER.lock() {
        *current = handler;
    }
}

struct HiddenControls(Option<VisibilityHandler>);
impl HiddenControls {
    fn acquire() -> Result<Self, String> {
        let handler = VISIBILITY_HANDLER
            .lock()
            .map_err(|_| "Capture controls lock failed.")?
            .clone();
        if let Some(handler) = &handler {
            if let Err(error) = handler(true) {
                // A delayed UI acknowledgement can arrive after the timeout.
                // Queue restoration so an abandoned capture cannot hide the
                // status item for the rest of the session.
                let _ = handler(false);
                return Err(error);
            }
            // Allow the status-item removal to reach the compositor.
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(Self(handler))
    }
}
impl Drop for HiddenControls {
    fn drop(&mut self) {
        if let Some(handler) = &self.0 {
            let _ = handler(false);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionStatus {
    Granted,
    /// The preflight API cannot distinguish never asked from a denied request.
    NotGranted,
    Unknown,
}

pub fn permission_status() -> PermissionStatus {
    #[cfg(target_os = "macos")]
    {
        #[link(name = "CoreGraphics", kind = "framework")]
        unsafe extern "C" {
            fn CGPreflightScreenCaptureAccess() -> bool;
        }
        // The read-only preflight does not request permission or capture pixels.
        if unsafe { CGPreflightScreenCaptureAccess() } {
            PermissionStatus::Granted
        } else {
            PermissionStatus::NotGranted
        }
    }
    #[cfg(not(target_os = "macos"))]
    PermissionStatus::Unknown
}

pub fn permission_guidance(status: PermissionStatus) -> &'static str {
    match status {
        PermissionStatus::Granted => "Screen recording permission is granted. If capture fails, select an available source and retry.",
        PermissionStatus::NotGranted => "Screen recording access is not granted to this running copy. Open Screen Access in Framecraft and choose Request Access. Then enable Framecraft in System Settings > Privacy & Security > Screen & System Audio Recording (Screen Recording on older macOS). If it is missing, use the + button to add /Applications/Framecraft.app. Quit and reopen that installed copy, then retry.",
        PermissionStatus::Unknown => "Framecraft could not determine screen recording permission. Check System Settings > Privacy & Security > Screen Recording, then retry explicitly.",
    }
}

/// Call only in response to the user's Request Access action, on the UI thread.
/// macOS owns the prompt and approval; a false result is not proof of denial.
pub fn request_permission() -> PermissionStatus {
    #[cfg(target_os = "macos")]
    {
        #[link(name = "CoreGraphics", kind = "framework")]
        unsafe extern "C" {
            fn CGRequestScreenCaptureAccess() -> bool;
        }
        if unsafe { CGRequestScreenCaptureAccess() } {
            PermissionStatus::Granted
        } else {
            permission_status()
        }
    }
    #[cfg(not(target_os = "macos"))]
    PermissionStatus::Unknown
}

pub fn require_permission(status: PermissionStatus) -> Result<(), String> {
    match status {
        PermissionStatus::Granted => Ok(()),
        _ => Err(permission_guidance(status).into()),
    }
}

pub fn open_permission_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("/usr/bin/open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
            .status()
            .map_err(|e| format!("Could not open System Settings: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err("Open System Settings > Privacy & Security > Screen & System Audio Recording manually.".into())
        }
    }
    #[cfg(not(target_os = "macos"))]
    Err("Screen recording settings are available on macOS.".into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DestinationFailureKind {
    Missing,
    NotDirectory,
    PermissionDenied,
    Full,
    Other,
}

#[derive(Debug)]
pub struct DestinationFailure {
    pub kind: DestinationFailureKind,
    pub message: String,
}

impl std::fmt::Display for DestinationFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

/// Probe with a uniquely-created temporary file; existing captures are never
/// opened or overwritten. This is a point-in-time check, not a disk-space promise.
pub fn probe_destination(path: &Path) -> Result<(), DestinationFailure> {
    use std::io::Write;
    let map = |error: std::io::Error| {
        let (kind, message) = match error.kind() {
            std::io::ErrorKind::NotFound => (DestinationFailureKind::Missing, "The destination no longer exists. Choose a folder."),
            std::io::ErrorKind::PermissionDenied => (DestinationFailureKind::PermissionDenied, "Framecraft cannot write to this folder. Choose another folder or fix its access permissions."),
            std::io::ErrorKind::StorageFull => (DestinationFailureKind::Full, "The destination is full. Free space or choose another folder, then retry."),
            _ => (DestinationFailureKind::Other, "The destination could not be checked. Choose another folder or retry after fixing storage."),
        };
        DestinationFailure {
            kind,
            message: format!("{message} ({error})"),
        }
    };
    if !std::fs::metadata(path).map_err(map)?.is_dir() {
        return Err(DestinationFailure {
            kind: DestinationFailureKind::NotDirectory,
            message: "The destination is not a folder. Choose a folder.".into(),
        });
    }
    let mut probe = tempfile::Builder::new()
        .prefix(".framecraft-write-check-")
        .tempfile_in(path)
        .map_err(map)?;
    probe
        .write_all(b"Framecraft destination check\n")
        .map_err(map)?;
    probe.as_file().sync_all().map_err(map)?;
    probe.close().map_err(map)
}

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
    capture_cancellable(mode, &|| false)
}

/// Cancellation affects only private capture staging. A cancelled selector is
/// killed and reaped before returning; its pixels never reach the editor.
pub fn capture_cancellable(
    mode: CaptureMode,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<RgbaImage>, String> {
    if cancelled() {
        return Ok(None);
    }
    #[cfg(target_os = "macos")]
    {
        run_capture(mode, &command_args(mode), cancelled)
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

/// Snapshot one display without opening a picker. The index is macOS's -D index.
pub fn capture_display(display: u32) -> Result<RgbaImage, String> {
    capture_display_cancellable(display, &|| false)
}

pub fn capture_display_cancellable(
    display: u32,
    cancelled: &dyn Fn() -> bool,
) -> Result<RgbaImage, String> {
    if !(1..=16).contains(&display) {
        return Err("Choose a display number from 1 to 16.".into());
    }
    #[cfg(target_os = "macos")]
    {
        let index = display.to_string();
        run_capture(
            CaptureMode::Screen,
            &["-x", "-t", "png", "-D", &index],
            cancelled,
        )?
        .ok_or_else(|| {
            if cancelled() {
                "Capture cancelled because the session changed.".into()
            } else {
                "The display did not produce a screenshot.".into()
            }
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = cancelled;
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

    pub fn capture_cancellable(&self, cancelled: &dyn Fn() -> bool) -> Result<RgbaImage, String> {
        self.crop(&capture_display_cancellable(self.display, cancelled)?)
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
fn run_capture(
    mode: CaptureMode,
    args: &[&str],
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<RgbaImage>, String> {
    use std::fs;
    use std::io::ErrorKind;
    use std::os::unix::fs::PermissionsExt;
    use std::process::{Command, Stdio};

    // Covers one-off and collection paths without prompting from a worker.
    // Permission denial must not hide controls or start a capture subprocess.
    if cancelled() {
        return Ok(None);
    }
    require_permission(permission_status())?;
    let _hidden = HiddenControls::acquire()?;
    if cancelled() {
        return Ok(None);
    }

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

    // A private file avoids blocking on a full stderr pipe. Diagnostics are
    // bounded when read and removed alongside the temporary screenshot.
    let diagnostic_path = temporary.path().join("diagnostic.txt");
    let diagnostic_file = fs::File::create(&diagnostic_path).map_err(|e| e.to_string())?;
    let mut child = Command::new("/usr/sbin/screencapture")
        .args(args)
        .arg(&path)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(diagnostic_file))
        .spawn()
        .map_err(|e| {
            format!("Could not start macOS screen capture: {e}. Check that /usr/sbin/screencapture is available.")
        })?;
    let deadline = (mode == CaptureMode::Screen).then_some(CAPTURE_DEADLINE);
    let Some(status) = wait_for_capture(&mut child, deadline, cancelled)? else {
        return Ok(None);
    };

    let file_exists = match fs::metadata(&path) {
        Ok(metadata) => metadata.is_file(),
        Err(error) if error.kind() == ErrorKind::NotFound => false,
        Err(error) => return Err(format!("Could not inspect the captured image: {error}")),
    };
    use std::io::Read;
    let mut diagnostic = Vec::new();
    fs::File::open(diagnostic_path)
        .map_err(|e| e.to_string())?
        .take(4096)
        .read_to_end(&mut diagnostic)
        .map_err(|e| e.to_string())?;
    let stderr = String::from_utf8_lossy(&diagnostic);
    if !classify_completion(mode, status.success(), file_exists, &stderr)? {
        return Ok(None);
    }

    // The shared decoder checks dimensions before allocating the full image,
    // applying the same 64-megapixel ceiling as Open Image.
    let image = crate::editor::load_image(&path)
        .map_err(|e| format!("macOS captured an image, but Framecraft could not open it: {e}"))?;
    Ok((!cancelled()).then_some(image))
}

#[cfg(target_os = "macos")]
fn wait_for_capture(
    child: &mut std::process::Child,
    deadline: Option<Duration>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<std::process::ExitStatus>, String> {
    let started = std::time::Instant::now();
    loop {
        if cancelled() {
            let _ = child.kill();
            child
                .wait()
                .map_err(|error| format!("Could not finish cancelling screen capture: {error}"))?;
            return Ok(None);
        }
        let reason = if deadline.is_some_and(|limit| started.elapsed() >= limit) {
            Some("Screen capture timed out. Session paused; check the source and permissions, then retry.")
        } else {
            None
        };
        if let Some(reason) = reason {
            let _ = child.kill();
            let _ = child.wait();
            return Err(reason.into());
        }
        match child.try_wait() {
            Ok(Some(status)) => return Ok(Some(status)),
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("Could not check screen capture: {error}"));
            }
        }
    }
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
        "Screen capture failed. {detail} No new screenshot was loaded. Open Screen Access to check permission for this copy. If access is granted, retry with an available display or a non-empty area. This command error alone does not identify a permission problem."
    ))
}

#[cfg(test)]
mod tests {
    use super::{classify_completion, command_args, CaptureMode};

    #[test]
    fn capture_requires_explicit_granted_status() {
        use super::{require_permission, PermissionStatus};
        assert!(require_permission(PermissionStatus::Granted).is_ok());
        assert!(require_permission(PermissionStatus::NotGranted)
            .unwrap_err()
            .contains("Request Access"));
        assert!(require_permission(PermissionStatus::Unknown).is_err());
    }

    #[test]
    fn rect_failure_does_not_claim_permission_denied() {
        let error = classify_completion(
            CaptureMode::Area,
            false,
            false,
            "could not create image from rect",
        )
        .unwrap_err();
        assert!(error.contains("could not create image from rect"));
        assert!(error.contains("does not identify a permission problem"));
        assert!(!error.contains("Terminal"));
    }

    #[test]
    fn pre_cancelled_capture_never_opens_native_selection() {
        for mode in [CaptureMode::Area, CaptureMode::Window, CaptureMode::Screen] {
            assert!(super::capture_cancellable(mode, &|| true)
                .unwrap()
                .is_none());
        }
    }

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

    #[test]
    fn destination_probe_leaves_no_files_and_rejects_a_file() {
        let temporary = tempfile::tempdir().unwrap();
        super::probe_destination(temporary.path()).unwrap();
        assert_eq!(std::fs::read_dir(temporary.path()).unwrap().count(), 0);
        let file = temporary.path().join("existing.png");
        std::fs::write(&file, b"keep me").unwrap();
        assert!(super::probe_destination(&file).is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"keep me");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn unattended_process_has_deadline_and_cancellation_without_screen_capture() {
        use std::process::Command;
        use std::time::{Duration, Instant};
        let mut child = Command::new("/bin/sleep").arg("5").spawn().unwrap();
        let began = Instant::now();
        assert!(
            super::wait_for_capture(&mut child, Some(Duration::from_millis(30)), &|| false)
                .unwrap_err()
                .contains("timed out")
        );
        assert!(began.elapsed() < Duration::from_secs(1));
        assert!(child.try_wait().unwrap().is_some());
        let mut child = Command::new("/bin/sleep").arg("5").spawn().unwrap();
        assert!(super::wait_for_capture(&mut child, None, &|| true)
            .unwrap()
            .is_none());
        assert!(child.try_wait().unwrap().is_some());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn completed_private_capture_can_be_cancelled_without_publication() {
        let mut child = std::process::Command::new("/usr/bin/true").spawn().unwrap();
        child.wait().unwrap();
        assert!(super::wait_for_capture(&mut child, None, &|| true)
            .unwrap()
            .is_none());
        assert!(super::wait_for_capture(&mut child, None, &|| true)
            .unwrap()
            .is_none());
    }
}
