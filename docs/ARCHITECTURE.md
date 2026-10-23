# Architecture: version 0.2

Framecraft is a Rust desktop application with a GPU-rendered egui interface. The default page is **Session**; the single-image **Editor** remains available. System capture and optional Apple Vision OCR sit behind Rust adapters. No Cap code is reused.

## Components

| File | Responsibility |
| --- | --- |
| `app.rs` | Application pages, display-selection snapshot, editor, file/clipboard actions, edited-copy export |
| `session_ui.rs` | Session setup, thumbnail list, rename/edit actions, focus-driven pause, native global shortcuts |
| `session_runner.rs` | Capture worker, scheduling, pause/resume/stop state, limits, separate bounded naming worker |
| `session.rs` | Session folders, atomic PNG persistence, safe filenames, rename, thumbnail change detection |
| `capture.rs` | System screenshot invocation, numbered-display capture, saved target geometry and crop validation |
| `naming.rs` | Apple Vision text recognition and deterministic heading ranking |
| `editor.rs` | Source-coordinate annotations, validation, crop, compositing, flattened output |

## Selecting and collecting

Selecting a session source hides the app briefly and captures the chosen display using `screencapture -D`. The snapshot opens in the editor's crop tool. Confirming the crop records the display index, physical-pixel rectangle, and source image dimensions. Confirming without a crop selects the entire display.

Each later sample captures that display again. The adapter checks dimensions and crops before the image reaches the session store or naming worker. A size mismatch pauses the session. Display indices are not stable identities: an equal-sized monitor replacing another index is not detected. There is no window following.

The current capture adapter writes a full-display screenshot into a private temporary directory, then decodes and crops it. Temporary cleanup is best effort. This is not an operating-system region-only capture implementation or a secure-erasure guarantee.

## Scheduling and UI control

The worker uses explicit running, paused, and stopped states, with a generation counter to invalidate old work even across a rapid pause/resume. Manual requests use a bounded command queue and retain their generation. Pause and Stop discard stale requests and captured pixels; an atomic persistence operation already begun is allowed to finish. Interval mode schedules the next sample after the previous capture work; configured intervals therefore do not guarantee an exact wall-clock cadence.

Change mode samples about once per second. It compares 128 × 72 greyscale fingerprints, counts pixels whose intensity changes by at least 16/255, and applies an adjustable changed-area threshold, settling period, and cooldown. The saved baseline advances only after persistence succeeds. Resume resets the detector and its baseline, so the last stable frame may be saved again after settling. It measures visual change, not semantic slide identity.

Successful Start and Resume minimise the app and allow a short settling delay before capture. Returning focus to Framecraft pauses the session. Global `Cmd+Option+S/P/X` shortcuts are registered on the main thread during an active session and polled with a 100 ms UI wake interval. Registration failure leaves the session paused with an error; Resume retries registration. Stop releases those registrations. Source selection and settings remain disabled while a session is active, including while paused.

The default limits are 500 images or 120 minutes since session start, including paused time. Capture or save errors pause the worker. These interactions need native runtime verification, especially while the app is minimised.

## Persistence and naming

The store creates a dated session directory without reusing an existing one. PNGs are encoded to a temporary file in that directory, synced, and persisted without clobbering an existing capture. Names contain a UTC timestamp and sequence. Rename preserves those fields, sanitises the title, and resolves collisions within the same directory.

A shared `CaptureHandle` serialises naming and image loading for each capture. Successful manual renames are authoritative: pending or stale OCR results cannot replace them, including when the user explicitly keeps the existing title. Background editor loading resolves and decodes the current path under the same per-capture lock, so an OCR rename cannot remove the file between path lookup and opening. Other captures use independent locks.

After saving, the capture worker publishes a thumbnail and optionally submits a bounded image to a separate OCR worker. The naming queue holds four pending jobs. A full queue skips the name suggestion, not the image save. OCR failure or missing meaningful text retains the fallback filename.

Apple Vision runs on the naming worker through Rust Objective-C bindings. Heading selection considers recognised text and its geometry. There is no language-model inference, cloud endpoint, or application model download. OCR text is bounded in memory and is not stored as a sidecar. Apple Vision itself is a proprietary macOS framework.

## Editing and export

Annotations use source-image coordinates independently of canvas zoom. Highlight rectangles use source-over alpha. Freehand strokes share the segment rasteriser with arrows and union stroke coverage so joins do not repeatedly apply alpha. Freehand input is limited to 10,000 finite, bounded points and widths of at most 512 pixels.

Redactions are applied last as opaque replacement before crop and styling. The flattened edited PNG contains no editable layers. The source image and undo state still exist in memory, and the session's unedited PNG remains on disk. **Save edited copy** adds a uniquely named PNG beside that original; it does not sanitise the original or the whole session directory.

The editor and CLI use the same compositor. Imported images become RGBA8; source colour profiles are discarded without colour conversion. HDR and wide-gamut fidelity are not implemented.

## Future capture adapter and release

A direct Rust `objc2` ScreenCaptureKit adapter would remove the command-line capture boundary and enable more precise source handling. `SCScreenshotManager` implies macOS 14 or newer; the current deployment target is macOS 13. Stable display identity, window targeting, mixed Retina scales, permission changes, and system-picker behaviour need explicit tests.

The bundle identifier is `dev.framecraft.app`. The local bundle script uses ad-hoc signing. Developer ID signing, notarisation, updates, and installation on a separate Mac remain release work. CI checks cannot validate a live meeting capture session.

## References

- [Cap product](https://cap.so/) and [repository licence](https://github.com/CapSoftware/Cap/blob/main/LICENSE)
- [egui/eframe](https://github.com/emilk/egui)
- [Apple SCScreenshotManager](https://developer.apple.com/documentation/screencapturekit/scscreenshotmanager)
- [Apple VNRecognizeTextRequest](https://developer.apple.com/documentation/vision/vnrecognizetextrequest)
- [objc2](https://github.com/madsmtm/objc2)
