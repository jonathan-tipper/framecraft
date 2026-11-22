# Capture-to-paste candidate 0.3.0-beta.6

22 November 2026. Checks ran on a MacBookPro18,2 with Apple M1 Max, macOS 26.6.2 (25G83) and Rust 1.95.0. Runtime commit `5f1448a` includes #30, #31 and #34 through integration commit `393686c`. The bundle identifies itself as `dev.framecraft.app`, build `3.0.6`, architecture `arm64`, macOS 13 minimum. The deployment target by itself does not verify macOS 13 runtime compatibility.

| Check | Observed result |
| --- | --- |
| Locked default-feature suite | 203 passed across library, app and integration tests. 5 opt-in tests were skipped in this run. |
| Locked core suite | 138 passed, 4 opt-in tests skipped. The suites overlap. |
| Build and analysis | Formatting, strict Clippy with and without default features, release build, plist lint and Git diff checks passed. The bundled executable exported a synthetic 1248 × 828 PNG. |
| Local paste at a caret | The explicitly run private-pasteboard fixture decoded every RGBA byte of a 7 × 3 image. A separate offscreen AppKit rich-text process inserted it between two words and checked the attachment’s dimensions and pixels. Cancellation after encoding and invalid input preserved the previous item. |
| Mac shortcuts | Two processes contended for Control–Option–F18. The second exclusive registration failed and acquired the chord after release. macOS also allowed an exclusive claim over a shared registration. The setup explains this limitation. A Carbon event addressed to the fixture's own application target produced exactly one Capture Area command. |
| Native menus and state | The Mac controls fixture exercised Edit Last Capture, retry, clear, app settings, guarded quit, session state updates and releasing registrations. Tests covered a failed binding/preference transaction, recording loss of focus, repeats, Escape, physical key position versus the displayed legend, and the menu-only choice. |
| Handoff and recovery | Tests covered retained dirty documents, exact PNG output, repeated selections, failed copy/save, retry, cancellation before publication, and clearing the last capture. Display-ID tests suppress an ambiguous or changed thumbnail location. |
| Native thumbnail and focus | The inactive fixture showed the production AppKit panel on a synthetic target. The panel stayed non-key and non-main, the foreground process and offscreen caret stayed the same, and Edit/Clear dismissed it. Hiding and restoring the synthetic editor preserved its previous alpha and mouse handling. CoreGraphics reported the physical display was asleep, so ordinary placement correctly suppressed the thumbnail. |
| Development DMG | The read-only mounted image passed its checksum. The enclosed app passed strict signature, layout and executable equality checks. Installation, replacement, refusal and injected rollback passed in temporary destinations. The existing application in Applications was left in place. |
| Existing native OCR regressions | Both opt-in Apple Vision tests passed on synthetic pixels, including the dark-interface fixture and redacted text. |

The [native thumbnail image](evidence/beta6-native/thumbnail.png) contains only the AppKit panel’s own view with a synthetic capture. It shows the receipt and both actions at 2×. The focus test ran from an inactive process. The image records appearance separately from the physical shortcut and screen-selection test below.

The local development DMG has SHA-256 `ad6d5a94055572f802be1afefdb63c9634865ea59b30f887ee0e44c1bceb79cc`. Independently built CI images carry their own checksum. The local signature reports `adhoc` and no team identifier. The Mac keychain returned zero valid code-signing identities. GitHub had no release environment, repository signing secrets or self-hosted runner configured. The distribution packager rejected missing, ad-hoc and unavailable identities. Apple notarisation and a Developer ID installation are pending in [#35](https://github.com/jonathan-tipper/framecraft/issues/35).

The private pasteboard, addressed Carbon event and native fixtures establish separate parts of the flow. The physical shortcut → system selector → immediate ⌘V sequence in a foreground document, actual mouse-release latency, second-Mac install/upgrade and consent, multiple physical displays, a second installed keyboard layout and VoiceOver remain open. Use the [MacBook test procedure](TESTING.md#beta-6-shortcut-and-paste-on-another-mac) with the exact distributed DMG.

# Everyday capture candidate: 0.3.0-beta.5

8 November 2026. Implements the text correction in #14, setup in #32 and visual consistency follow-ups in #27/#28. This is a local test candidate, not a published or accepted release.

## Automated checks

- `cargo test --workspace --all-targets --locked`: **187 passed**, 4 explicitly ignored (127 library, 49 application, 11 pixelation tests).
- `cargo test --workspace --no-default-features --locked`: **134 passed**, 4 ignored.
- Strict Clippy passed for all targets with default and no-default features. Formatting and diff checks passed.
- Text regressions exercise focused typing/Escape, add/edit/cancel, blank/oversized validation, atomic undo/redo, unchanged Apply, modal command guards and shared composed output.
- Capture preference/storage tests cover round-trip, invalid/corrupt settings, key conflicts, unsafe prefixes/paths, exact original pixels, concurrent filename collisions, cancelled staging and publication boundaries. App tests retain a failed-save image as unsaved, mark a successful auto-save clean, and protect subsequent edits.
- Setup renders all 4 steps headlessly at 900 × 620. A separate accessibility-bounds test checks Open/Close/Copy/Save/Save Edited Copy/More Actions in a dirty session-frame header at that width. Rendering without a panic is not treated as visibility proof.
- Review found and corrected cancellation during PNG staging, an unused default key reserved by skipped setup, hidden permission feedback from a background trigger, and conflicts with OCR/prepared-drag dialogs. Strict Clippy caught an unbound accessibility label; it was connected and the check rerun successfully.

## Native observations

A separately identified, ad-hoc-signed `Framecraft Preview.app` was launched with synthetic sample content. The installed `/Applications/Framecraft.app` was not replaced and no Screen Recording permission was changed.

- Walked through Access → Shortcut → Files → Finish; the naming preview and edit/save choices appeared. Not now returned to capture-ready, and the next launch offered setup again. Finish was not used to change the user's preferences.
- Selected Text, clicked the sample image and typed immediately into the focused field. Add text created a visible label. Edit text prefilled the original, Escape discarded new wording, reopening confirmed the original, and Apply changed the rendered label.
- Saved an edited sample through the native PNG picker; inspected the resulting 1120 × 700 PNG and observed the revised text. The disposable path was `/private/tmp/Screenshot-editedframecraft-beta5-text-native.png.png`; the picker automation appended rather than replaced its name, and the app preserved the picker-approved path.
- Visual inspection exposed low contrast in the inherited blue-on-dark default. New labels now default to white on the dark backing; a final rebuilt preview confirmed this visibly.
- Resized the final native preview to 900-point width and its minimum content height. Tool buttons share one width, the tool rail scrolls, capture/edit actions remain visible, text entry fits, and the nudge controls use a regular 2-column grid. The dirty session-specific header is covered by the bounds test; its native session walkthrough remains open.
- Command-Q after editing showed Save/Discard/Cancel. Discard closed the disposable sample; process inspection confirmed the preview exited.

Evidence: [setup](evidence/beta5-native/setup.jpg), [text entry at minimum size](evidence/beta5-native/text-edit.jpg), [final compact editor](evidence/beta5-native/editor.jpg). Setup/text images show actual native UI, not a browser mock-up. They do not establish VoiceOver usability or user preference.

## Build and remaining acceptance

Release build, strict ad-hoc signature and plist checks passed. DMG creation required the host disk-image service outside the restricted shell; the host packaging run and checksum verification passed. Local artefact: `dist/Framecraft-0.3.0-beta.5-macos-arm64.dmg`, SHA-256 `c4fb93a8c396f9a75c70ccb0834857659ff37ea2546b4b30423bf98535e55e63`.

Still required: Finish/relaunch preference verification through the native app, real background shortcut registration/conflict and permission recovery, selector completion/Escape through automatic saving, VoiceOver and keyboard-only setup, non-Retina/multiple displays, supported macOS versions/Intel and a user comparison of the design. The existing app permission recovery evidence below remains historical; a new ad-hoc binary may need renewed consent. Developer ID signing/notarisation remain outstanding. #32 and the related stories stay open for these acceptance gaps. Persistent editable documents are backlog #33.

---

# Screen access recovery verification: 0.3.0-beta.4

1 November 2026. Fix for the reported `could not create image from rect` failure and inability to enable Framecraft. Source inspection found the preflight helper unused, no direct screen-capture permission request, and generic permission advice appended to every command failure. This establishes a missing recovery flow, not the cause of every rectangle failure or the affected Mac's TCC state.

- `cargo test --locked --all-targets`: 166 passed (116 library, 39 app, 11 pixelation), 4 explicit tests ignored.
- `cargo test --locked --no-default-features`: 123 passed, 4 ignored.
- Strict Clippy passed with default and no-default features; formatting and diff checks passed.
- Regression checks cover granted/not-granted/unknown gating, preservation of an edited document without starting capture, no automatic capture after permission checks, and rectangle diagnostics that do not assert permission denial.
- Existing unsaved-capture routing test now injects permission status, so it does not depend on the test runner's real macOS permission. The initial run exposed that dependency; the deterministic retest passed.
- Release bundle, strict ad-hoc signature and plist checks passed. DMG verification passed: `Framecraft-0.3.0-beta.4-macos-arm64.dmg`, SHA-256 `f3d1008bcca0995e434b9452301ef58d2d5f097632d6f4653b94e4903e5313bc`. The existing installed app was not replaced.
- Native FFI declarations were checked against the installed macOS SDK. `CGRequestScreenCaptureAccess` is invoked only by the explicit Request Access button on the UI thread. Session/capture workers only preflight.

The automated checks did not change permissions, reset TCC or capture screen content. Native recovery was subsequently checked as recorded below. The local build is ad-hoc signed; stable distribution identity and notarisation are still outstanding. Unit tests do not prove approval works on another Mac.

## Confirmed native permission recovery, 1 November 2026

On macOS 26.6.2, System Settings showed Framecraft enabled while the app preflight reported no access. The macOS privacy log explicitly reported `Failed to match existing code requirement for subject dev.framecraft.app and service kTCCServiceScreenCapture`, with different saved and current code-directory hashes. The running process was the repository's `dist/Framecraft.app` copy. This confirms a signature mismatch in this incident; the rectangle error alone had not established it.

The empty development instance was quit normally. After explicit user approval, `tccutil reset ScreenCapture dev.framecraft.app` succeeded. No other app's permissions were reset. `/Applications/Framecraft.app` was opened, its process path checked, and Request Access invoked. After reapproval/relaunch, the installed app's native preflight reported **Screen recording permission is granted**. [Native status evidence](evidence/beta4-native/access-granted.jpg).

No source or binary rebuild was needed for this recovery. Actual area/window selection and capture after recovery have not yet been observed; do not equate a successful permission check with a completed capture. An ad-hoc rebuild that changes the binary can change its signing identity and require new consent.

---

# Native editor development verification: 0.3.0-beta.3

31 October 2026. This is unreleased implementation work for #23–#28, following the [native editor contract](specs/editor-native-experience.md). The installed beta 2 and its historical results below remain separate. No new installation, release, general macOS compatibility or completed story acceptance is claimed by this section.

| Check | Recorded result |
| --- | --- |
| Focused capture adapter tests | `cargo test --locked --lib capture::tests`: 13 passed. Includes pre-cancelled operations, child cancellation/reaping, timeout and repeated cancellation after private completion. Synthetic process tests use sleep/true; they do not open a native selector. |
| Combined locked all-targets and no-UI tests | `cargo test --locked --all-targets`: 163 passed (114 library, 38 application/UI, 11 pixelation integration); 4 explicit tests ignored. `cargo test --locked --no-default-features`: 121 passed, 4 ignored. Includes transaction rollback/undo, dirty state, save failure/stale acknowledgements, modal guards and hidden-pixel/preview-output invariance. |
| Formatting and strict Clippy | `cargo fmt --all -- --check` and strict Clippy (`--all-targets -- -D warnings`) passed with default and no-default features; diff whitespace passed. |
| Release build, bundle/plist/signature | Release bundle generated at `dist/Framecraft.app`, version 0.3.0-beta.3/build 3.0.3, with ad-hoc signing. Strict signature verification and plist lint passed on the rebuilt bundle. |
| Local test DMG | `Framecraft-0.3.0-beta.3-macos-arm64.dmg` built and `hdiutil verify` passed. SHA-256 `4fc836108d45213647544615d617076957c8fd1c99dfdfdf915228026d71ce89`. Final bundled executable SHA-256 `efd0c8d601767f2750cb8350188b965ddaeb638fdbf281bbfa362c8c0de11dc9`, built from implementation commit `7e1b10bb8cdc8d930b5dcdbb51872e4d0abb42d6`. Local test artefact only; no GitHub release published. |
| Release compositor timing | Explicit ignored 4K test run passed: 32.04, 26.06 and 26.09 ms for full-image Pixelate 12 plus Redact on this arm64 macOS 26.6.2 machine. This measures compositor work, not end-to-end UI latency. |
| Native editor layout and control interaction | Ran the built app with synthetic content at its default 1080 × 760 content size. Rail, contextual inspector, named swatches, selected state and close decision inspected. Corrected overflowing inspector rows and action-versus-toggle accessibility roles. Native 900 × 620 check remains pending. |
| Native Area/Window completion and pointer/Escape cancellation | Pending. A menu entry or process test does not prove that the system selector allows pointer interaction. |
| Native property transaction and close/save recovery | Added Highlight and Pixelate regions; colour-dialog Cancel retained the mark; Close/Cancel retained edits. Typed block-size validation, Escape and Undo passed. Actual native Save published a PNG and Close returned to capture-ready. Failure/race paths remain automated evidence; held-pointer Escape and full focus traversal remain pending. |
| Native accessibility, VoiceOver and appearance/scale | Pending; no conformance claim. |
| Participant comparison against beta 2 | Not performed. No improved-usability measurement is claimed. |

The new capture adapter preserves private staging and hidden capture controls. The application enables cancellation only for its active one-off capture and retains the previous document on cancellation. Native pointer reachability is a material open check because the existing tray is hidden during capture and macOS owns selection input.

## Native smoke record, 31 October 2026

Used the local beta 3 bundle on arm64 macOS 26.6.2 with its built-in synthetic sample, not private screen content. Screenshots: [selected highlight](evidence/beta3-native/highlight.jpg), [unsaved close decision](evidence/beta3-native/close.jpg).

- Chose Green, added a centred highlight, opened Custom colour and cancelled. The selected mark and unsaved document remained. This checks dialog cancellation, not a complete colour-preview/VoiceOver study.
- Added a pixelation region. Double-clicked the numeric field to focus it, committed 12 → 48, then typed 999. Typing passed through valid 99, which previewed; final 999 showed validation. Escape restored the transaction's opening 48. Undo, followed by reselecting the region, showed 12. The region remained present.
- Opened Close screenshot and cancelled; the same edited image remained. A later Close → Save PNG opened the actual macOS save panel. Saved synthetic output to `/private/tmp/framecraft-beta3-native-smoke.png`; the app returned to capture-ready and stayed running. Read back the PNG header: 1120 × 700 pixels, 27,547 bytes, SHA-256 `aa8f382cb941d66c019151c1ed1816251598e50d1e9e0e056274fb3f096289ae`. This is file-existence/header evidence, not a new full output-parity test.
- Actual AX inspection identified command buttons incorrectly marked as toggles and missing selected swatch state. Rebuilt controls expose ordinary Capture/Copy/Save/Close buttons and selected named swatches. Inspector combo/nudge/footer overflow was corrected and visually retested. Final AX inspection confirmed the selected-mark popup is named “Select mark”. These observations do not establish VoiceOver conformance.

No native screen selection, permission changes, receiving-app paste/drag, minimum-size window, session-return or participant comparison was performed in this smoke run. The user-installed beta 2 was not replaced. Source tests cover additional cases but do not fill these native evidence gaps.

Extend this section with observed results, build identity, hardware and remaining failures before describing a test build as verified. The [native story matrix](TESTING.md#native-editor-story-acceptance) is the procedure, not evidence of completion.

---

# Verification notes: 0.3.0-beta.2

Checked 29 October 2026 on the same macOS 26.6.2 arm64 machine as beta 1 below. Issue [#20](https://github.com/jonathan-tipper/framecraft/issues/20) records the usability correction.

| Check | Result |
| --- | --- |
| Locked all-target tests | 138 passed: 112 library + 26 application/UI; 3 existing explicit native/long tests ignored |
| Locked tests without UI | 108 passed; 3 existing explicit checks ignored |
| Formatting, strict Clippy, diff whitespace | Passed |
| Native controls smoke | Passed actual Area/Window/Screen menu routing, disabled busy capture, settings/quit, shortcut registration and native termination protection |
| Installer regression script | Passed staged install, explicit replacement, wrong-app/symlink/running refusal, unrelated-file retention and failed-publication rollback. Process enumeration and publication failure were simulated; signatures/filesystem operations were real |
| Local installation | Installed at `/Applications/Framecraft.app`, build 3.0.2; strict ad-hoc signature verification passed |
| Initial screen and settings | Installed app launched to Screenshot with visible Area/Window/Screen commands. Mac system font and light surfaces inspected. Shortcut dialog showed Command/Option checkboxes, key choices and scope explanations; Save/Close were visible |
| Application menu | Native Capture menu displayed Area/Window/Screen, with inactive collection actions disabled |
| Shortcut draft and background controls | In the installed final build, Sessions controls disabled behind settings; clearing the last modifier retained a usable binding; Close/reopen discarded an unsaved change and restored the saved binding |
| Final DMG | Created, checksum-verified, mounted read-only and signature-checked; Applications link and beta version present; bundled binary matched the installed application |

The native area command launched `/usr/sbin/screencapture -i -s` and hid Framecraft as intended. The automation could not deliver Escape or selection input to the system selector while the app was hidden. Two test-only selectors were explicitly terminated without saving an image; the app returned with an explained error. A synthetic Preview image was used for the second attempt. **Actual rectangle completion and ordinary Escape cancellation remain unverified**. This tool limitation is not a successful capture result. No permission was granted or changed by automation.

New regression tests cover initial single-capture state, dialog/busy capture gating, unsaved-edit prompts retaining the intended action, unmodified image dimensions and shortcut drafts staying separate from committed session bindings. Existing hidden-editor, redaction/output, session persistence and worker checks still pass.

The broader permission, multi-display, external receiver, VoiceOver, Intel and older-macOS matrix below remains outstanding. The session-first initial form and raw shortcut entry are retired. Beta 1 evidence below is historical, not a claim that every native check was repeated for beta 2.

## Historical beta 1 verification

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
