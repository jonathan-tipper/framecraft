# Framecraft 0.2

A Rust screenshot workbench for macOS, built around capture sessions. Select the useful part of a meeting or presentation once, collect frames without reopening a selector, then annotate the ones worth sharing.

This is an early implementation. [Cap](https://cap.so/) is already open source and uses substantial Rust; Framecraft's distinction is its session workflow and all-Rust application UI. No Cap code is reused. There is no recording, hosted sharing, account system, or subscription.

## Session workflow

1. In **Session**, choose a title, destination folder, and display number.
2. Click **Select shared area**. Framecraft takes a display snapshot; use **Crop** to mark the content, then **Use this area**. **Use full display** is also available.
3. Choose **On shortcut**, **On a timer**, or **When content changes**, then **Start session**. Framecraft minimises.
4. Collect images while using your meeting or presentation app. Returning to Framecraft pauses capture so you can review thumbnails, rename files, or choose **Edit**.
5. Add arrows, rectangles, highlights, freehand ink, opaque redactions, or a crop. **Save edited copy** writes another PNG in the session folder and keeps the original.

See [the workflow examples](docs/WORKFLOW.md) for filenames and operating limits.

| Capture mode | Behaviour |
| --- | --- |
| On shortcut | `Cmd+Option+S` saves the selected region while the session is running |
| On a timer | Configurable 5–120 second interval; capture and processing time affect the actual cadence |
| When content changes | Samples about once a second and saves a sufficiently changed, settled frame; threshold and 0.5–5 second settling period are adjustable |

The default limits are 120 minutes of elapsed session time, including pauses, or 500 saved images, whichever comes first. Stop the session before changing its source area or settings.

The area is a fixed rectangle in the selected display's screenshot pixels. It does not follow a window. A display-size change pauses capture; reordering equal-sized displays is not detected. Stop and select again after moving the content or changing your display setup.

## Names and saved images

Each image is saved immediately with a UTC timestamp, sequence number, and fallback title. Optional background Apple Vision OCR looks for a visible heading and renames the file afterwards. This is OCR plus heading-selection rules, not a local language model or a general image captioner. Framecraft makes no cloud calls or model downloads for naming.

OCR failure, missing text, or a full naming queue leaves the saved PNG with its fallback name. Files are not dropped because naming is slow. You can rename frames yourself; your chosen name takes priority over pending OCR. Names are sanitised and collisions do not replace existing captures. Opening a frame for editing is synchronised with background filename changes.

## Editor

The **Editor** tab also supports one-off area, window, and primary-display capture; PNG/JPEG imports; undo/redo; PNG export; image copying; gradients; padding; rounded corners; and shadows. Export is a flattened image, not an editable project.

| Shortcut | Action and scope |
| --- | --- |
| `Cmd+Option+S` | Capture selected region, global during a running session |
| `Cmd+Option+P` | Pause/resume, global while a session is active |
| `Cmd+Option+X` | Stop session, global while a session is active |
| `Cmd+Option+4`, `5`, `3` | One-off area, window, primary-screen capture while the editor is focused |
| `Cmd+O` | Open image |
| `Cmd+S` | Save edited copy for a session frame; otherwise open PNG export |
| `Cmd+Shift+C` | Copy composed image |
| `Cmd+Z`, `Cmd+Shift+Z` | Undo, redo |

Global shortcut registration can fail if a combination is unavailable. The app reports that failure and leaves the session paused. Successful Start and Resume minimise the window; returning to it pauses collection. Resume resets change detection, so the last stable frame can be saved again. Pause and Stop invalidate in-flight capture work, but an atomic file save already underway can finish. These native behaviours still need testing on a Mac.

## Build and run

Target: macOS 13 or newer, with a stable Rust toolchain and Apple's command-line developer tools.

```sh
cargo run --release

# Build a local application bundle.
./scripts/bundle-macos.sh
open dist/Framecraft.app
```

The script uses the lockfile, when present, and creates an ad-hoc signed bundle with identifier `dev.framecraft.app`. This is local development signing, not Developer ID signing or notarisation.

macOS may request Screen Recording permission for Framecraft, Terminal, or the IDE launching it. Grant access to the process identified by macOS and restart it if requested.

```sh
# Editor sample and headless exports.
cargo run --release -- --demo
cargo run --release -- --demo-export /tmp/framecraft-demo.png
cargo run --release -- --render input.png output.png

# Source checks.
cargo fmt --all -- --check
cargo test --all-targets
cargo test --no-default-features
cargo clippy --all-targets -- -D warnings
```

The CLI export commands do not create capture sessions. The current source passes 65 tests on Linux, formatting, Linux Clippy, and Apple silicon macOS cross-target Clippy. A linked macOS application and native runtime behaviour remain unverified. Exact commands, limitations, and the desktop test checklist are in [test notes](docs/TEST-NOTES.md).

## Privacy and platform boundaries

Session originals are saved to the chosen folder before editing. **Redacting an edited copy does not remove the unredacted original in that folder.** Share the edited file deliberately. Undo and the in-memory source also retain the original pixels. There is no automatic edit-history or OCR-text sidecar saving.

Capture currently uses `/usr/sbin/screencapture`. Each sample briefly captures the whole selected display to a private temporary file, then crops it before session saving and OCR. Cleanup is best effort, not secure deletion. Clipboard images and any folder synchronisation are subject to the usual behaviour of other apps on the Mac.

Images are converted to RGBA8. Wide-gamut and HDR fidelity are not implemented; source ICC profiles are discarded without a colour-management transform.

The application UI and compositor are Rust, using GPU-rendered [egui/eframe](https://github.com/emilk/egui) controls rather than AppKit widgets. There is no webview or JavaScript UI. The application's original source is MIT licensed; macOS capture and optional Apple Vision OCR are proprietary operating-system components. Dependency licence metadata is in [the inventory](docs/DEPENDENCIES.md), which is not a manual upstream-source audit.

[Architecture](docs/ARCHITECTURE.md) · [Product direction](docs/PRODUCT.md) · [Cap's licence](https://github.com/CapSoftware/Cap/blob/main/LICENSE)
