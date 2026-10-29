# Verification notes: 0.3.0-beta.1

Checked on 28–29 October 2026 on macOS 26.6.2 (25G83), Apple silicon (M1 Max, 64 GB), Rust 1.95.0. The release binary is arm64, ad-hoc signed, with `LC_BUILD_VERSION minos 13.0` and SDK 26.5. These results support a local beta test, not a general compatibility or notarisation claim.

## Automated and native harness results

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed |
| `cargo test --all-targets --locked --offline` | 131 passed: 109 library + 22 application/UI; 3 explicit native/long tests ignored in this ordinary run |
| `cargo test --no-default-features --locked --offline` | 105 passed; the same 3 explicit checks ignored |
| `cargo clippy --all-targets --locked --offline -- -D warnings` | Passed |
| Two explicit `naming::tests::native_` tests | Passed outside the tool sandbox with synthetic light/dark pixels only; redacted/cropped text absent |
| `synthetic_change_mode_benchmark_three_runs` | Explicit run passed: 3 repeats, 104 samples per repeat, 22/30 intended frames saved, 8 misses, no duplicates or transition saves |
| `examples/native-controls-smoke` | Passed outside the sandbox: native status menu, 3 shortcut registrations, state changes, unregister; actual `NSApplication.terminate(None)` returned a queued Quit without replacing winit's delegate |
| `bash scripts/bundle-macos.sh` | Built a linked release `.app`; ad-hoc signing applied |
| `codesign --verify --deep --strict --verbose=2` | Valid on disk; designated requirement satisfied |
| `plutil -lint`, shell syntax and `git diff --check` | Passed |
| Synthetic CLI fixtures | Created 10 originals + 10 registered edits and a separate 10-PNG/index pack without capturing the desktop |

The benchmark's limits and labelled misses are recorded in [change-mode evidence](evidence/change-mode.md). It is a deterministic synthetic corpus, not evidence of real meeting completeness or user benefit.

New regressions cover running/paused source guards, old source-result rejection after a start/stop cycle, hidden editor shortcut isolation, persistence conflicts/recovery, missing edited variants without fallback, bounds/path traversal, stale drag/OCR results, edit replacement protection, current-revision previews, output pixel equality, neutral names, HTML escaping, atomic pack publication and cancellation. Worker tests exercise actual save/naming flow with injected backends, including blocked persistence, queued naming and stop/drain ordering. Failed shortcut registration cannot be bypassed with Resume.

## Native UI observations on this Mac

The signed bundle was launched with synthetic files only. No desktop, meeting or private content was captured during these checks.

- Reopened a ten-frame session using the native folder picker, then relaunched and reopened it from Recent sessions. The session stayed stopped, variants survived and the source required reconfirmation.
- Copy/Reveal/Drag and pack selection stayed disabled until an output variant was explicitly chosen.
- Opened a saved edited variant at 1248 × 828 with zero added padding/radius/shadow. The existing styling was not applied again.
- The first dark-image OCR attempt returned punctuation. That reproduced failure led to bounded OCR-input contrast adjustment. The rebuilt bundle read `Synthetic review frame 1` and its step number in the review dialog. The source/export pixels were unchanged.
- Copy image reported completion. The attempted receiving-app check in Preview did not produce a document through the automation interface; cross-app paste is therefore **not verified** by this run.
- Changed the editor style and pressed Cmd+Q. The app remained open, displayed Save/Discard/Cancel and disabled editor mutations. Cancel retained the document. A later Discard exited; a fresh launch showed stopped initial state, and a final clean Cmd+Q left no Framecraft test process.
- Selected one saved edited frame and opened the final evidence-pack preview. It showed one PNG + `index.html`, a neutral title/folder and the chosen edited image. The destination chooser was opened; UI publication was not completed in this run. The separate CLI pack and atomic writer tests passed.

These checks do not establish VoiceOver usability or external drag compatibility. The automation's accessibility listing truncates long lists; screenshot inspection confirmed the pack dialog when its nodes were outside that listing.

## Remaining release and acceptance gates

Keep the related GitHub issues open until their remaining criteria have evidence:

- Real capture permissions: denial, grant/restart, revocation and failure recovery. No full permission cycle was performed.
- Standard/Retina and multiple displays, equal-size reordering, rotation, unplugging, moving content, timer/manual/change capture, and exclusion of Framecraft controls from captured pixels.
- Real global-key use while another app has focus, conflict recovery, minimisation/restore, Dock Quit, logout/shutdown, and forced termination. Programmatic native termination and Cmd+Q were checked; force quit cannot guarantee persistence.
- Actual copy/paste and drag/drop into a browser ticket form and a native document app; delayed-drop file lifetime and Finder failure cases.
- Native keyboard-only traversal and VoiceOver, cancellation during large folder loads, removable/network volumes and actual full-disk failures.
- Pack inspection on a second Mac while offline; native UI export/retry/cancel across chosen destinations.
- macOS 13 and other supported-version runs, Intel build/runtime, clean-Mac installation, Developer ID signing and notarisation.
- Memory/performance measurements, wide-gamut/HDR fidelity, the human pilot [#9](https://github.com/jonathan-tipper/framecraft/issues/9), and scrolling demand [#18](https://github.com/jonathan-tipper/framecraft/issues/18).

The beta preserves a fixed display-region adapter. [The targeting ADR](decisions/0001-capture-targeting.md) records why stable source/window migration remains open. Session renaming requires hard links; unsupported filesystems fail without deleting the original. Local atomic replacement, fsync and file locking are the supported storage contract.

Use [TESTING.md](TESTING.md) to extend this record. Record failures and unperformed checks alongside passes; do not infer completion from feature implementation. Version 0.2's historical Linux evidence remains in Git history.
