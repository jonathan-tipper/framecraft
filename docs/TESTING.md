# Testing the 0.3 beta

This is the test procedure, not a results report. Put actual commands, outcomes, commit, OS, hardware and failures in [TEST-NOTES.md](TEST-NOTES.md). An unchecked native case remains unverified even when its unit tests pass.

## Automated checks

From the repository root with a stable Rust toolchain:

```sh
cargo fmt --all -- --check
cargo test --locked --all-targets
cargo test --locked --no-default-features
cargo clippy --locked --all-targets -- -D warnings
cargo clippy --locked --no-default-features --all-targets -- -D warnings
cargo build --locked --release
```

Use `--offline` only when every locked dependency is cached. Failures caused by a missing cache are not test failures and are not passes. Build and inspect the actual app bundle after source checks:

```sh
./scripts/bundle-macos.sh
codesign --verify --deep --strict dist/Framecraft.app
plutil -lint dist/Framecraft.app/Contents/Info.plist
open dist/Framecraft.app
```

Signing verification confirms the ad-hoc signature's integrity; it does not establish notarisation, clean-Mac installation or runtime compatibility.

The library tests cover source-coordinate crop/redaction, alpha blending, Unicode fallback, all-type annotation selection/movement, OCR reading order and bounds, capture generations, file naming races, manifest recovery, variant equality and staged output publication. Consult the actual test inventory and result counts rather than treating this description as evidence.

## Native synthetic OCR

These explicit tests generate their own pixels. The light fixture crops one secret label and redacts another; the dark interface fixture verifies its label is recognised, then verifies a redaction removes it:

```sh
cargo test --locked --no-default-features --lib \
  naming::tests::native_ \
  -- --ignored --nocapture
```

It performs no desktop capture. Both recorded checks passed on macOS 26.6.2, arm64, outside the restricted tool sandbox. Inside that sandbox, CoreVideo pixel-buffer creation failed. The dark fixture initially returned punctuation instead of its visible title; bounded contrast normalisation now recognises the label. Do not interpret these English synthetic fixtures as broad language, accuracy, privacy or platform validation. The exact contrast sampling policy is documented in [architecture](ARCHITECTURE.md#ocr-boundaries).

## Synthetic review fixtures

Generate disposable review and output fixtures on the test Mac without capturing the desktop:

```sh
cargo run --locked --no-default-features -- --demo-session /tmp/framecraft-test-data
cargo run --locked --no-default-features -- --demo-pack /tmp/framecraft-test-data
```

The session command prints a new dated folder containing 10 original PNGs, 10 edited variants and session metadata. Open that folder in Framecraft to exercise reopening, variant choice and handoff. The pack command creates `Framecraft-sample-pack` under the parent, containing 10 flattened PNGs and `index.html`; an existing destination is rejected. Copy it to another Mac for the offline check. All content is synthetic. These fixtures provide test inputs, not evidence that native drag, clipboard or accessibility behaviour has passed.

## Desktop smoke checklist

Use a synthetic presentation with labelled corners, a changing slide number and synthetic sensitive text. Prepare a second display if available. Start with a disposable local folder and preserve a copy before corruption/recovery tests. Record build/commit, hardware, macOS version, display count, resolution/scale and launch method.

| Check | Expected result |
| --- | --- |
| Fresh launch and permission denial | Useful screen-access guidance; no false saved-frame acknowledgement |
| Grant/revoke access and retry | Recovery follows the named process; denial after success remains explicit |
| Cancel area/window selector | Clean cancellation; no blank or partial saved capture |
| Non-default display and Retina region | Labelled source pixels match the confirmed rectangle |
| Move/resize content | Fixed-region limitation is visible; no claim of window following |
| Resize/reconfigure display | Dimension mismatch pauses; equal-sized swaps remain a documented limitation |
| Active/paused source controls | Cannot silently replace the active worker's target |
| Manual, timer and change modes | Saved sequence is correct; distinguish missed, duplicate and transitional frames |
| Pause/resume/stop during a capture | Stale work cannot restart collection; an already-started atomic save may finish |
| Focus return/minimisation | Review pauses capture; resume minimises; no app chrome in saved region |
| Menu bar controls and visibility | State/count/source are coherent; controls hidden during capture as intended |
| Shortcut conflict/reconfiguration | Error visible; rollback does not silently lose prior bindings; stopped-only changes |
| Quit/relaunch | No capture restarts automatically; stored preferences and recent folders behave as documented |
| Read-only/full destination | Actionable error; existing PNGs retained; capture pauses on persistence failure |
| Slow/failed OCR naming | PNGs acknowledged independently; fallback name retained; manual rename wins |
| Open session and variants | Saved original/edited pixels match files without second styling |
| Move session folder | Reopen works from new location; stale recent reference can be forgotten |
| Legacy import cancel/confirm | Preview lists recognised files; cancellation changes nothing; confirmation is repeatable |
| Interrupted/invalid metadata | Saved PNGs survive; recovery is explicit; invalid metadata backup is retained |
| Unknown schema and concurrent writer | Refusal without destructive migration or concurrent writes |
| Missing/modified/symlink images | Explicit state; no silent original fallback or outside-file import |
| Select/revise annotations | Every tool moves/deletes; obscured marks selectable; undo/redo units are useful |
| Text/steps at crop/canvas edges | Pixels match preview/export; `?` fallback and limits are visible |
| Redaction plus overlapping marks | Covered source remains absent from PNG, copied image, drag and pack |
| Unsaved quit/open/replace | Save/export, discard and cancel work; failed export preserves edits |
| OCR preview/cancel/revision change | Explicit Copy only; old/failed/empty results do not alter clipboard |
| Native drag into Finder and target apps | Chosen variant imports; neutral PNG name; cancellation is safe; no raw-original fallback |
| Reveal saved file | Finder selects the loaded variant; unsaved changes are not claimed to be saved |
| Ten-frame offline pack on another Mac | Exact selected order/captions/variants and file count; images resolve without Framecraft/network |
| Pack collision/cancellation/disk failure | No existing destination overwritten and no apparently completed partial pack |
| Keyboard/VoiceOver | Controls are named and reachable; selected state, dialogs and recovery are understandable |
| Long session/resource use | Track RSS, temporary storage, queue bounds and responsiveness; record leaks/failures |

The native receiving-app matrix, older supported macOS versions, multiple Macs and VoiceOver checks are outstanding until results are recorded. No numerical latency/RSS target is claimed as achieved.

## Output inspection

For a redacted selected pack, enumerate every output file: one `index.html` plus exactly the selected PNGs. Confirm neutral filenames, no original capture names/paths, no OCR transcript, no session manifest, no extra original and no unselected frame. Inspect PNG metadata as well as visible pixels. Use a hostile caption such as `<script>alert(1)</script>` and confirm it is rendered as text in the offline HTML.

Decode each output PNG and compare RGBA pixels with its frozen preview snapshot. A saved edited variant must not gain a second border/shadow. Try a destination symlink into the source folder, an existing empty folder and an interrupted export. Retain the user's draft after failure.

## Product and detector evidence

The [pilot in #9](https://github.com/jonathan-tipper/framecraft/issues/9) needs consented target users and comparison tasks; do not replace it with developer smoke tests. Pre-register metrics and report assistance, failures and sample limits. [Scrolling discovery #18](https://github.com/jonathan-tipper/framecraft/issues/18) depends on that evidence and is not a beta feature.

[Change-mode evaluation #12](https://github.com/jonathan-tipper/framecraft/issues/12) uses labelled synthetic transitions/noise/animation and reports misses, duplicates and transitional saves separately. Real meeting completeness and under-one-second performance aspirations need measured runs on named hardware. Record distributions and failures rather than only the fastest sample.


## Beta 2 first-use acceptance

1. Install the DMG into Applications, eject it, then launch the installed copy. Confirm the app and menu bar icons and native Framecraft/Capture/Window menus.
2. Without visiting Sessions or Shortcuts, click Capture Area. Draw around a synthetic image in another application. Check the returned pixels, dimensions, Copy Image and Save PNG. Repeat with Escape; the previous document must survive.
3. Add an annotation, minimise the app, then invoke Capture Area from the menu bar. The restored window must show Save/Discard/Cancel. Cancel must retain the edit. Do not perform this check with private desktop content.
4. Open Keyboard Shortcuts. Change a modifier/key, close without saving, reopen and confirm the committed binding remains. Check duplicate/reserved combinations, a registration conflict and Restore Defaults. Uncheck the last modifier: the row must stay editable with a valid modifier selected.
5. Start a collection. One-off capture commands must disable; session Capture/Pause/Stop must continue using committed bindings. Confirm returning to review pauses collection as documented.
6. Resize to the minimum window and use keyboard only. Confirm settings can scroll to Save/Close and that the screen behind the dialog cannot start a collection. Record VoiceOver separately; the presence of AX nodes is not conformance evidence.
7. Test permission denial/grant/restart using the installed app, not a development launcher. Record the exact OS wording and identity; do not bypass security protections in test scripts.

See TEST-NOTES.md for what was actually performed. A selector process starting does not establish that selection/cancellation succeeded.

## Native editor story acceptance

This matrix covers #23–#28 against [the native contract](specs/editor-native-experience.md). Use the actual beta 3 development binary with synthetic inputs. Record its commit, bundle version, OS/hardware, display scale and launch method. Browser prototype screenshots cannot satisfy these cases.

| Case | Procedure and expected evidence |
| --- | --- |
| Fresh capture versus saved import | Close a new unannotated capture: Save/Discard/Cancel must appear. Copy first and repeat: the decision must remain. Open a clean saved PNG and close: it may return directly. |
| Close is separate from Quit | From a dirty standalone image, test each close decision. Cancel retains the exact edits; Discard leaves the app capture-ready; completed Save retains the file and closes only that image. Repeat capture without relaunching. |
| Save cancellation/failure | Cancel the native picker, then use a disposable unwritable destination. Both preserve current edits; cancellation is not reported as a failed save. Retry succeeds without losing the intended pending action. |
| Save identity and revision | Automated tests inject a completion for an old document/revision. It must not acknowledge or close newer edits. Exercise the real save-and-close path separately. |
| Session-origin close | Open a chosen variant from a stopped session, close it and verify return to the same review context. Compare original/edited files and manifest before/after; closing cannot alter them. |
| Gesture cancellation | Hold a highlight/freehand/crop/move gesture, press Escape before release, then release. Starting geometry and history must remain. Repeat with release first, then Undo. |
| Mode exit and nested Escape | Cancel tool retains marks. Escape in a colour editor or nested control must not close the screenshot. Verify field/popover precedence and focus return, not only the top-level dialog. |
| Native selector cancellation | Try Area and Window with both Escape and the pointer-accessible cancellation route. Record whether the native selector permits menu interaction. Repeat after minimising; prior visibility and document must return. A test-only process termination is not a successful pointer test. |
| Capture exclusion and cleanup | Complete a real synthetic capture and inspect whether Framecraft's controls appear. Cancel during native selection and retry immediately; no lingering child or abandoned capture should replace the editor. |
| Highlight target and transaction | Set Next to Green, draw, select and change to Pink. Next remains Green. Preview custom RGB, Cancel, reopen and Apply, then Undo. Original value, one undo step and useful focus return must be observed. |
| Pixelation target and transaction | Draw two regions; change only the selected one. Start at 12, commit 48, type an invalid value, Escape and Undo. Verify the last committed value and useful undo boundaries; no invalid value reaches rendering. |
| Pixelation rendering | Verify coarse/fine changes, 2 × 2 minimum, clipped edges, source-pixel units at zoom, overlapping regions and Redact. Unit tests cover deterministic alpha/grid and hidden-source invariance; inspect actual exported PNG separately. |
| Output parity and originals | Compare the composed reference to PNG, clipboard and prepared drag pixels. Reopen the saved flattened result; check OCR input and session/pack output routes. Originals remain byte-identical. Native receiver testing remains separate from compositor tests. |
| Minimum and large window | At 900 × 620 logical points and a larger size, inspect capture/copy/save/close, all tool names, inspector scrolling and every dialog decision. Repeat supported display scales and OS appearance/contrast settings. Save actual native screenshots. |
| Icon/state vocabulary | Explain Capture/Crop, Pixelate/Redact and Cancel/Close before activating. Inspect idle/hover/pressed/focus/selected/disabled/busy/error states. Do not substitute arbitrary Unicode or missing-font symbols. |
| Keyboard and VoiceOver | Traverse tools, properties, cancellation and closing without a pointer. Record accessible names, values, selected state, focus return and duplicate announcements. An AX-tree listing alone is insufficient. |
| Formative comparison | Run the same predefined tasks on beta 2 and the candidate with a new participant. Record completion, wrong actions, assistance, familiarity and design changes. Do not infer improved usability from developer checks. |

For pixelation, unit fixtures must vary only hidden redacted/cropped source samples and compare all visible output pixels. Include transparent pixels with differing hidden RGB, partially transparent averages, block origins, clipped edges, overlap ordering and aggregate work limits. Resource-bound rejection should produce an actionable error rather than silently lowering quality.

For UI changes, source or headless checks support regression diagnosis but do not establish native pointer interaction, VoiceOver or rendering at Retina scale. Keep every unperformed item explicit in TEST-NOTES and the owning issue.

## Screen access recovery, beta 4

Use an installed copy and record macOS version, build and executable path. Use synthetic screen content. Do not reset other applications' permissions.

1. With access unavailable, click Capture Area. Framecraft must stay visible, retain the current image/edits and show Screen Access without starting a selector.
2. Choose Request Access. Confirm the macOS prompt names the intended installed copy. The tester chooses Allow or denial; Framecraft cannot approve it.
3. Denial must preserve the document. Check Again reads status without capturing. Open System Settings must open the recording privacy pane or show a useful fallback error.
4. If absent, add `/Applications/Framecraft.app` using the privacy list's + button. Approve access, quit/reopen when requested, Check Again and explicitly retry capture. Test both Area and Screen with synthetic content.
5. Record whether an ad-hoc update or different launch path requires approval again. Test the actual installed build, not only cargo/Terminal attribution.
6. A failed/empty rectangle with granted access must retain its command diagnostic and must not assert that permission was denied. Original images remain unchanged.

Record first-grant, denied, revoked, restart-required and already-granted cases separately. Managed-device restrictions and native system prompts need an actual Mac test, not mocks.


## Everyday capture and text correction (beta 5 candidate)

Use a disposable destination and synthetic image. Record app path, signing identity, OS version and whether each check was automated or observed natively.

1. First launch shows 4-step setup. Visit each step at 900 × 620, use Back, then Not now. Confirm capture/import remains available and no preferences were saved. Reopen Capture setup from header, app menu and Command-comma.
2. Configure a spare shortcut and a disposable folder, prefix and UTC date grouping. Finish, quit and reopen; confirm values persist. Try conflicting session/OS keys and an unwritable folder. Failure must retain the draft and prior working key. Malformed quick-capture settings must be reported before any explicit replacement.
3. From another app, press the global shortcut. With permission denied, Framecraft must come forward and explain access. With access granted, select an area. Escape cancels. Check active-session, dirty-image and open-dialog guards.
4. Open-editor mode writes no automatic file. Automatic mode saves original pixels under the configured UTC name and opens them for optional edits. Repeat within the same second; earlier PNGs remain byte-identical. Delete/move the disposable folder and retry; capture must remain open and unsaved with manual Save PNG recovery. Cancel before atomic publication creates no PNG; a completed save is retained.
5. Open the synthetic sample. Choose Text, click the image and type without another click. Add a multiline label, select Edit text, change it and Cancel/Escape. Reopen and verify original wording. Apply a real change, Undo/Redo and save a PNG. Inspect exported pixels. New text should be white against its dark backing by default; custom colours remain explicit.
6. Repeat on an imported PNG and a session frame. A dirty session-frame header at minimum width must retain Copy, Save, Save Edited Copy and Close access. Existing mark types, crop and redaction output must remain intact. Exported/reopened PNGs do not restore editable text objects.
7. Check keyboard traversal, named controls and VoiceOver; record assistance needed and any unclear icons. Native appearance at Retina/non-Retina and supported macOS versions still needs separate runs.
