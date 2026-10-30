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
