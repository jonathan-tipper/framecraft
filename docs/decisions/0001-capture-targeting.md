# ADR 0001: Keep macOS 13 while investigating stable source identity

Date: 28 October 2026. Status: discovery recommendation, production migration deferred. Source of truth: [#8](https://github.com/jonathan-tipper/framecraft/issues/8); dependencies [#5](https://github.com/jonathan-tipper/framecraft/issues/5) and [#7](https://github.com/jonathan-tipper/framecraft/issues/7).

Framecraft 0.3 retains `screencapture`, the selected display index, physical-pixel region and expected source dimensions. It adds cancellation and a 10-second deadline for unattended calls. It does **not** solve equal-sized monitor reordering or follow a moving window. Stop and reselect after a display arrangement changes. A different size or scale already fails the target check; an equal-sized substitute cannot currently be distinguished. This remains a known limitation, not a claim of stable targeting.

## Evidence

Apple's [ScreenCaptureKit](https://developer.apple.com/documentation/screencapturekit) model exposes displays, windows and applications through [SCShareableContent](https://developer.apple.com/documentation/screencapturekit/scshareablecontent). Content filters can select a display or a desktop-independent window. Apple's [sample](https://developer.apple.com/documentation/screencapturekit/capturing-screen-content-in-macos) now requires macOS 15, so copying its deployment settings would break Framecraft's baseline.

Availability was checked against Apple's installed SDK headers under `$(xcrun --show-sdk-path)/System/Library/Frameworks/ScreenCaptureKit.framework/Headers` on this machine, as well as the linked documentation. These are Apple declarations, not successful runtime tests on every supported OS.

| API | Declared minimum macOS | Consequence |
| --- | --- | --- |
| `SCShareableContent`, `SCDisplay.displayID`, `SCWindow.windowID`, `SCContentFilter`, `SCStream` | 12.3 | A stream-based backend can retain macOS 13. |
| `SCWindow.isActive` and `SCStreamFrameInfoScreenRect` | 13.1 | Must not be assumed on 13.0. |
| `SCScreenshotManager.captureImage`, `SCContentSharingPicker`, filter `pointPixelScale`/`contentRect` | 14.0 | A still-image manager or system picker requires availability checks and a 13.x path. |
| Filter `includeMenuBar` | 14.2 | Cannot provide universal menu exclusion on the current baseline. |
| `SCScreenshotManager.captureImage(in:)` | 15.2 | Not a macOS 13 replacement. |
| New screenshot configuration/output APIs | 26.0 | Outside the current baseline. |

[CGPreflightScreenCaptureAccess](https://developer.apple.com/documentation/coregraphics/cgpreflightscreencaptureaccess()) is declared available from macOS 10.15. Its Boolean result does not identify whether permission was never requested or explicitly denied. Framecraft reports “not granted”, retains the Terminal/IDE attribution caveat for developer launches, and does not claim capture will work until a capture succeeds.

## Options and judgement

| Approach | Benefits | Costs and limits | Recommendation |
| --- | --- | --- | --- |
| Improve existing subprocess adapter with a verified display-identity mapping | Small implementation surface, current 13.x flow remains available, isolated per-capture files | Must prove `-D` ordering against CoreGraphics identity; indices and pixel dimensions alone are insufficient. Check topology before and after capture, and discard on mismatch. Still pays process and PNG decode costs. | First prototype. Do not ship mapping based on assumed enumeration order. |
| Direct ScreenCaptureKit, using `SCStream` on 13 and optionally `SCScreenshotManager` on 14+ | Explicit window/display identity, content filters, in-memory pixel delivery, potential window tracking | Asynchronous stream lifecycle, frame completeness, point-to-pixel conversion, queues, cancellation, permission recovery and two OS paths need real tests. IDs must be reacquired and validated; never assume a window ID survives closure/relaunch. | Preferred direction once the native matrix passes. Keep a capability boundary rather than changing the editor's RGBA contract. |
| Raise the minimum to 14/15 and use only newer screenshot/picker APIs | Less compatibility code and a system selection surface | Excludes supported Macs, does not itself prove source continuity, needs an explicit product decision | Rejected for 0.3. No authorised baseline increase. |

## Required prototype cases

| Scenario | Desired contract | Evidence in this change |
| --- | --- | --- |
| Equal-sized displays swap order or reconnect | Resolve the same verified physical source, or pause and require reselection | Not run. Existing index-only adapter remains limited. |
| Display removed or scale changes | Pause; never substitute another display or enlarge the region | Synthetic dimensions/crop tests pass. Native disconnect and mixed Retina tests not run. |
| Window moves/resizes | Choose explicit fixed-region versus window-content semantics before implementation | Not run. Current region remains fixed in display pixels. |
| Window closes/minimises/is occluded | Handle unavailable/incomplete content without saving a blank acknowledgement | Not run. ScreenCaptureKit behaviour needs per-OS fixtures. |
| Permission changes during a session | Pause with explicit retry; no background resume from Settings | Injected capture failure and generation tests pass. Native deny/grant/revoke/relaunch not run. |
| Control exclusion and external content | Hide/exclude Framecraft controls and preserve selected-source boundaries | Main-thread hide acknowledgement implemented. Pixel contamination and other-app focus checks not run. |

The available machine is a MacBookPro18,2 with Apple M1 Max, 64 GB, macOS 26.6.2 (25G83), arm64, Rust 1.95.0. No second physical display or older/Intel Mac was used for this investigation. The bounded research result is an API comparison and a testable migration contract; it is not a completed multi-display prototype. Issue #8 must remain open.

## Migration, rollback and retained paths

1. Prototype identity acquisition and topology changes without reading unrelated desktop content. Record actual display identity, enumeration and scale behaviour with a synthetic fixture on two equal-sized monitors.
2. Add a source descriptor separating fixed display-region intent from window-content intent. Resolve identity for each session; pause on uncertainty. The UI must show the resolved source before collection.
3. Implement ScreenCaptureKit behind `CaptureAdapter`, retaining the existing RGBA output and worker generation checks. A compatibility fallback must refer to the same verified source; never silently broaden to a whole display.
4. Compare native latency, complete-frame rate, temporary storage and lifecycle against the same labelled corpus. In-memory ScreenCaptureKit avoids the subprocess temporary PNG in principle; no latency or memory benefit has been measured here.
5. Only then propose retirement of index-only selection. Roll back by selecting the existing adapter explicitly, invalidating active targets and requiring reselection; preserve every acknowledged PNG.

Retired now: unbounded unattended subprocess waiting and direct, uninjectable worker dependence on native capture/OCR. Retained: interactive `screencapture`, private temporary PNG decode, display-index/region contract, macOS 13 baseline and the existing local editor. No production backend migration or new source picker is included.
