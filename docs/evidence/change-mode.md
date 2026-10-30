# Change-mode evidence: synthetic baseline

Date: 28 October 2026. Sources of truth: [#12](https://github.com/jonathan-tipper/framecraft/issues/12) and the worker verification in [#7](https://github.com/jonathan-tipper/framecraft/issues/7). Build: 0.3.0-beta.1 working tree. Machine: MacBookPro18,2, Apple M1 Max, 64 GB, macOS 26.6.2 (25G83), arm64, Rust 1.95.0.

**Recommendation: keep the defaults and narrow the promise.** Change mode can collect broad, settled visual changes; it does not identify slide meaning or guarantee every text/colour edit. Use Capture now for a specific moment. This is an engineering benchmark using synthetic pixels, not a user study and not a live display measurement.

## Reproduce

```sh
cargo test --locked --no-default-features --lib synthetic_change_mode_benchmark_three_runs -- --ignored --nocapture
cargo test --locked --lib session_runner::tests -- --nocapture
```

`src/session_runner.rs::tests::benchmark_corpus` generates all pixels and ground-truth labels locally, with no random seed or external file. The ignored benchmark takes about 2 minutes. Three workers run the same 104-sample sequence concurrently, each using its own temporary session folder, actual worker scheduling, actual PNG writes and manifest persistence. The injected capture adapter returns synthetic frames; it never invokes screen capture. OCR is disabled. All temporary output is removed at the end.

Configuration is unchanged: 128 × 72 greyscale fingerprint, pixel intensity difference at least 16/255, changed fraction 0.04, settle 800 ms, cooldown 2 seconds, nominal sample cadence 1 second, poll 100 ms, initial arm delay 700 ms. Work performed between samples adds to the cadence. Measurements below include the worker's PNG/manifest work and test observation time; they exclude native capture, full-resolution screenshot decoding and OCR.

## Expected-output manifest

| Labels | Intended content | Input samples | Expected evaluation |
| --- | --- | --- | --- |
| 0–19 | 20 alternating broad slides, including returning to an earlier appearance, 4:3/16:9 changes and a small moving cursor/video-tile patch | One white transition, then 3 slide samples per label: 80 samples total | Each intended label should be saved once; transition labels must not be saved. |
| 20–23 | Four text-like narrow strip edits occupying less than 4% of the image | 3 samples per label: 12 total | Intended edits for completeness measurement. Known detector misses must be reported. |
| 24–26 | Red, green with near-equal greyscale intensity, then red again | 3 samples per label: 9 total | Intended colour edits; exposes greyscale information loss. |
| 27–29 | Three rapid grey levels, each visible for 1 sample | 3 total | Intended moments shorter than reliable settling; count misses separately from transitions. |
| 255 | Pure white intermediate transition | Included in first row | Any saved label 255 counts as a transitional save, never an intended frame. |

A single marker pixel identifies each label for output reconciliation. It is too small to meet the 4% area threshold. The patch simulates nuisance motion only; it is not a real cursor, video codec or meeting layout. “Duplicate” means more than one saved PNG for the same intended label. A returned slide has a new intended label and is not a duplicate.

## Results

The final verification passed all three repeats in 113.27 seconds, producing the same saved/missed labels. The initial result assertion expected all 3 rapid moments to miss; that hypothesis was wrong. Label 28 was saved because the consecutive 7/255 greyscale step was below the 16/255 pixel-change boundary, so it inherited the previous candidate's settling period. The regression expectation was corrected to preserve the observed baseline, without changing detection logic. An earlier harness attempt used a 5-second event timeout and aborted during the deliberately missed sequence; that harness error was fixed to allow 45 seconds without a save event.

| Run | Intended labels saved | Missed labels | Duplicate saves | Transitional saves | Observed label-to-save latency |
| --- | --- | --- | --- | --- | --- |
| 1 | 22/30 | 8/30 | 0/22 saved PNGs | 0/20 transition inputs | 31–1,151 ms |
| 2 | 22/30 | 8/30 | 0/22 saved PNGs | 0/20 transition inputs | 37–1,129 ms |
| 3 | 22/30 | 8/30 | 0/22 saved PNGs | 0/20 transition inputs | 31–1,138 ms |

Missed labels in every run: **20, 21, 22, 23, 25, 26, 27, 29**. Broad settled slides: **20/20 saved**, with **0/20 transition inputs saved**. Colour-only sequence: **1/3 saved**. Small text-like edits: **0/4 saved**. Rapid moments: **1/3 saved**. No acceptance target has been substituted for a measured result.

Latency starts when an intended label is first supplied by the synthetic capture adapter and ends when its saved event has been received and its PNG reopened. The short minimum comes from rapid label 28 inheriting the previous candidate timer; it is not a claim of a 31 ms native screenshot. These ranges are per-run extrema across saved intended labels, not percentiles or an estimate for missed frames.

## Manual capture, pause/resume and failure evidence

The normal worker test `manual_request_in_change_mode_bypasses_settle_and_resume_resets_baseline` verifies that a deliberate request saves the first sample in change mode. Pause/Resume then needs two stable samples and can save the same pixels again because the baseline resets. This is the documented Resume duplicate, separate from unexpected duplicates in the uninterrupted corpus.

Other real-worker tests cover stale results after rapid pause/resume; stop cancellation; a full 8-command manual queue; blocked, failed and saturated naming (4 waiting jobs plus one running); successful durable PNG acknowledgement before OCR finishes; and actual destination failure. Captures are never acknowledged merely because a key was pressed. An acknowledged PNG remains on disk even when OCR fails or its queue is full.

CPU utilisation, resident-memory sampling and native screenshot latency were **not collected**. The three concurrent runs also share the machine with development activity. The tiny fixtures exercise the scheduling and persistence contract; they do not establish CPU, memory, full-resolution save throughput or OCR capacity for a meeting. The naming-queue test demonstrates bounded scheduling and preserved PNGs under deliberately blocked recognition, not a throughput claim.

## Native coverage and remaining limits

| Check | Result | Evidence / limit |
| --- | --- | --- |
| Generated corpus through injected worker, three repeats | Passed for reproducibility; completeness limits measured above | 104 samples per run, 30 intended labels, actual disk persistence. |
| Broad static-slide target | Passed in synthetic corpus | 20/20; this is a narrower subset than all intended moments. |
| Resume duplicate and manual change-mode override | Passed | Normal worker regression test, no native capture. |
| Unattended process deadline/cancellation | Passed with `/bin/sleep` child fixture | 30 ms test deadline and immediate cancellation kill/reap the child; production limit is 10 s. No screen content read. |
| Destination preflight | Passed | Unique private temporary file, sync, cleanup, missing/file destination errors. Real full disk and OS permission denial not run. |
| Native status item and shortcut lifecycle | Passed in synthetic smoke app | `cargo run --locked --example native-controls-smoke`: menu construction, 3 registrations, active edit rejection, four status updates, release and close. No capture or OCR. |
| Native focus/fullscreen/menu interaction and control pixel exclusion | Not run | Hide acknowledgement is implemented; visual contamination still needs a labelled screen fixture. |
| Native deny/grant/revoke, bundled/developer attribution, OCR | Not run | Preflight Boolean is not a permission history. Test both launch contexts. |
| Retina/multiple displays, equal-sized reorder, Intel, macOS 13 | Not run | See [targeting ADR](../decisions/0001-capture-targeting.md). |
| External drag destinations and VoiceOver | Not run | Native drag helper compiles; keyboard Copy/Export remain available. |

The native smoke initially could not connect to macOS services inside the filesystem sandbox. It was terminated and rerun outside the sandbox, where the listed lifecycle checks passed. That rerun did not change privacy permissions or read desktop pixels.

Retired claims: “one image per meaningful slide” is unsupported. Retained defaults: no threshold, settling or cooldown tuning was made. The next experiment should use labelled full-resolution presentation windows on the supported OS/display matrix, inspect discrepant pixels, and measure native capture/OCR/save load independently before changing defaults.

## Native Quit guard follow-up: 29 October 2026

Integration review found that winit 0.30.13 wires its native Quit item and Command-Q to `NSApplication.terminate`, which does not pass through the window-close guard. Framecraft now adds the optional `applicationShouldTerminate:` method only when absent on the existing `WinitApplicationDelegate` class. It never replaces the delegate object, changes its class or overwrites another implementation. The callback returns `NSTerminateCancel` and coalesces requests into `DesktopAction::Quit`; the app then applies its save/discard decision and waits for worker drain before closing its viewport. The documented AppKit contract is [applicationShouldTerminate](https://developer.apple.com/documentation/appkit/nsapplicationdelegate/applicationshouldterminate(_:)) and [terminate](https://developer.apple.com/documentation/appkit/nsapplication/terminate(_:)).

The guard installs before fallible menu and shortcut setup. Once installed, a missing/disconnected handler still cancels native termination. Menu setup failure is reported by the app, and the ordinary window-close route remains available. An unexpected application delegate or a foreign existing termination method produces an explicit error instead of replacing framework behaviour. This integration is tied to the locked winit version and must be checked when upgrading it.

Verification on the same named Mac: the callback regression passed cancellation, repeated-request coalescing and missing-handler behaviour. `cargo run --locked --offline --example native-controls-smoke` invoked the actual `NSApplication.terminate(None)` path, received exactly one Quit action, verified that the delegate pointer was unchanged, remained alive through subsequent updates and then closed normally. No screenshots, OCR, user-settings writes, physical key synthesis or real system logout occurred. Clicking Dock Quit, actual Command-Q with an edited document, system shutdown and the complete save/discard interaction remain separate native checks.

Global shortcut validation also rejects Command-Q/O/S/Z, Command-Shift-Z/C, Command-Option-3/4/5 and native Hide bindings. Modifier aliases and ordering are normalised before this check. A regression first reproduced the collision and then passed after validation was added. These reservations prevent one keypress from reaching both an active global session action and an existing app command; other application/OS registration conflicts still use the native conflict path.
