# Verification notes: version 0.2

Native macOS runtime behaviour is unverified. Cross-target type-checking, unit tests, and headless layout checks do not establish that capture, OCR, minimisation, or global shortcuts work in a real desktop session.

Version 0.2 was checked on a Linux host with Rust 1.99.1. The Apple silicon target was checked from Linux, without macOS linking or execution.

| Check | Current evidence |
| --- | --- |
| `cargo fmt --all -- --check` | Passed |
| `cargo test --all-targets --locked` | 65 passed: 58 library tests and 7 application tests |
| `cargo test --no-default-features --locked` | 58 library tests passed |
| `cargo clippy --all-targets --locked -- -D warnings` | Passed on Linux |
| `cargo clippy --target aarch64-apple-darwin --all-targets --locked -- -D warnings` | Passed cross-target checking; no macOS linking or execution |
| Native Apple Vision OCR | Not executed on a Mac |
| Native capture session and global shortcuts | Not executed on a Mac |
| Linked application bundle | Not built or launched on a Mac |

The passing tests cover source-over highlights, bounded rectangles, freehand points and widths, single-point strokes, joins, clipping, final opaque redactions, crop bounds, output dimensions, persistence, filename collisions, UTC timestamps, visual settling, cooldown, noise, and heading-ranking rules.

Five `CaptureHandle` tests cover manual-name priority, stale OCR notifications, keeping an explicitly chosen existing title, rejecting stale automatic names, and serialising image loads with filename changes without blocking other captures. Eight runner tests cover state generations, rapid pause/resume, stale capture errors, arming, manual requests, and limits. Application tests cover editor state, export behaviour, and headless layout; these are not native visual or accessibility tests.

The Apple-target check reports a future-incompatibility warning in the transitive dependency `block 0.1.6`. The current checks pass; that dependency needs attention before a future compiler makes the warning an error.

## Required desktop validation

| Area | Manual check and expected outcome |
| --- | --- |
| Permissions | Deny screen access, grant it later, restart if requested, and confirm actionable errors without silent saves |
| Region selection | Select a known rectangle on standard and Retina displays; compare saved pixel boundaries with the selected snapshot |
| Multiple displays | Select each numbered display, including mixed scales; verify size changes pause capture; verify and document the equal-sized reordering limitation |
| Content movement | Move and resize the meeting window; confirm capture remains at the fixed region and requires reselection |
| Start and resume | Confirm minimisation occurs before capture; check the app does not appear in its own captured region |
| Focus and review | Restore Framecraft during each capture mode; confirm capture pauses and resumes only when requested; check change mode can save the last stable frame again after resume |
| In-flight work | Rapidly pause/resume and stop during capture; old requests and pixels must be discarded, while an atomic save already begun may finish |
| Global shortcuts | Exercise `Cmd+Option+S/P/X` while another app is focused and Framecraft is minimised; confirm conflicts leave the session paused and registrations release after Stop |
| Mode timing | Check manual, interval, and change modes against a labelled slide sequence; count missing, duplicate, and transitional frames |
| Limits and failures | Check small duration/count limits, pause time, permission loss, unavailable displays, and unwritable or full destinations |
| OCR queue | Test readable headings, diagrams, non-English text, OCR failure, and a slow recogniser; every acknowledged PNG must remain saved |
| Rename and edit | Rename during pending OCR, open frames for editing, save several edited copies, and confirm originals and unrelated files survive |
| Redaction | Inspect the exported edited PNG at full resolution; confirm covered pixels are opaque and sharing does not include the original by mistake |
| Desktop integration | Check native file dialogs, clipboard paste into other apps, GPU presentation, keyboard traversal, and VoiceOver |
| Release | Build and install on another Mac; test Apple silicon and Intel separately before making support claims |

Performance, memory use, wide-gamut colour, HDR fidelity, signing, and notarisation are not verified. Source images are converted to RGBA8 without an ICC colour transform. The supplied GitHub macOS workflow is a build definition, not evidence of a completed CI run.
