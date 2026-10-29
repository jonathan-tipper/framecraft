# Developer guide

Use this guide with the [architecture](ARCHITECTURE.md), [0.3 specification](specs/0.3-session-workbench.md) and the linked GitHub issue. The repository is a Rust application, not a JavaScript/webview wrapper.

## Local setup

Install a current stable Rust toolchain and Apple's command-line developer tools on a Mac. The intended deployment baseline is macOS 13. The current beta's native evidence is narrower; consult [test notes](TEST-NOTES.md). `Cargo.lock` is committed; use `--locked` for repeatable dependency resolution.

```sh
cargo run --locked
cargo run --locked --release -- --demo
cargo run --locked --no-default-features -- --demo-export /tmp/framecraft-demo.png
cargo run --locked --no-default-features -- --demo-session /tmp/framecraft-test-data
cargo run --locked --no-default-features -- --demo-pack /tmp/framecraft-test-data
cargo run --locked --no-default-features -- --render input.png output.png
```

`--demo` opens generated sample content. `--demo-export`, `--demo-session`, `--demo-pack` and `--render` use generated or explicitly supplied images, without screen capture. `--demo-session DIR` creates a new dated review session under DIR with 10 originals and 10 saved edited variants. `--demo-pack DIR` publishes an 11-file offline pack under `DIR/Framecraft-sample-pack` on macOS; use another parent or rename the existing pack before rerunning. Both create the supplied parent directory if needed. These fixture commands do not start a capture worker. `--render` applies default presentation styling and writes a new PNG. The `ui` feature is enabled by default; without it, headless image commands and library tests remain available. Native capture, OCR, menu controls and pack publication are macOS boundaries, not portable feature promises.

Building the bundle:

```sh
./scripts/bundle-macos.sh
open dist/Framecraft.app
```

The script builds the host release binary with the lockfile and `MACOSX_DEPLOYMENT_TARGET=13.0` by default, then creates an ad-hoc signed app with identifier `dev.framecraft.app`. Use the intended host architecture; do not mistake a cross-target compilation for runtime validation. The script is not a notarisation or Developer ID release pipeline.

## Change workflow

1. Identify the GitHub issue, its acceptance criteria, non-goals, dependencies and **Retires** section. For this release, [#1](https://github.com/jonathan-tipper/framecraft/issues/1) links the scoped delivery items and the [specification](specs/0.3-session-workbench.md) records decisions spanning modules.
2. Inspect repository instructions and current branches. Use a work-type branch such as `feat/session-review` or `fix/capture-source`; do not push directly to `main`.
3. Add a failing behavioural test for changed contracts, then implement the smallest slice. Keep native boundaries injectable so automated tests use synthetic data.
4. Update the linked issue/spec if intended behaviour changes. Do not silently weaken an acceptance criterion because a native check is inconvenient.
5. Run the relevant tests, then required release checks. Update user documentation, privacy/storage implications and verified/unverified evidence. Open a reviewable PR; a human makes the merge decision.

No production network service or model token budget is involved. Resource costs still matter: image buffers, preview snapshots, OCR queues and temporary output files must stay bounded.

## Contracts to preserve

- Capture source confirmation and the worker's target agree. Hidden Editor shortcuts cannot mutate a Session review document.
- Acknowledged PNG persistence survives OCR and metadata failures. Manual titles outrank stale naming results.
- Session paths are relative and validated. Unknown manifest versions are rejected. Preview cancellation does not modify the folder.
- All annotation coordinates refer to the original source. Redactions are opaque and last; crop/style follow them.
- Preview and output use the same compositor. Saved variants are already flattened and must not be styled again automatically.
- Full-text OCR sees only current rendered pixels. Revision/cancellation checks precede user-visible results and clipboard changes.
- Copy/drag/pack use the resolved snapshot. Missing edited variants never fall back to originals. Output names and default captions do not inherit private filenames or OCR text.
- Pack publication is exclusive and staged, outside source folders. Export failures preserve the draft and original data.
- PNG export does not save editable layers. Unsaved-change decisions remain explicit before quitting/replacing a document.

## Useful entry points

| Work | Starting point |
| --- | --- |
| Pixel rendering, annotation geometry | `src/editor.rs` and its pixel fixtures |
| OCR extraction/ranking | `src/naming.rs`; synthetic native smoke test |
| Files, recovery and capture identity | `src/library.rs`, `src/session.rs` |
| Runner races and scheduling | `src/session_runner.rs` and injected worker fixtures |
| macOS capture/permissions | `src/capture.rs` and process boundary tests |
| Menu bar and shortcut replacement | `src/desktop.rs`, `src/settings.rs` |
| Variant output and offline packs | `src/output.rs`, integration in `src/app.rs` |
| UI/review flow | `src/app.rs`, `src/session_ui.rs` |

Read the complete module contract before changing a shared path. In particular, `CaptureHandle` protects image loading from concurrent rename, while runner generation checks protect against stale capture work. Neither can substitute for the other's invariant.

## Dependencies and licensing

Use `Cargo.lock` to review version changes and [DEPENDENCIES.md](DEPENDENCIES.md) for declared licence metadata. The inventory covers optional and target-specific crates, not just the macOS binary. Recreate it after dependency changes; do not hand-edit version claims without resolving the lockfile.

```sh
cargo metadata --locked --format-version 1 > /tmp/framecraft-metadata.json
```

Bundled font notices are in [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md). Keep them with distributed test bundles. Do not swap in system fonts for exported labels without revisiting deterministic rendering, missing glyphs and licence obligations. Metadata declarations and included font notices do not constitute a complete manual source audit of dependencies.

## Verification and diagnosis

Run the commands and native matrix in [TESTING.md](TESTING.md). Keep exact results in [TEST-NOTES.md](TEST-NOTES.md), including failures, OS/hardware and any sandbox constraints. Do not log image content, OCR transcripts or private source paths for routine debugging. Reproduce defects with synthetic fixtures where possible.

Full-text OCR uses bounded contrast normalisation only on its rendered working buffer; see the exact sampling policy in [architecture](ARCHITECTURE.md#ocr-boundaries). Test both the dark-interface and light crop/redaction fixtures after changing this path.

Native OCR may fail inside a restricted execution sandbox because CoreVideo cannot allocate a pixel buffer. An explicitly authorised unsandboxed synthetic test distinguishes that environment limitation from OCR behaviour; it does not justify capturing a user's desktop or widening normal app permissions. Run only the named native OCR tests, not all ignored tests blindly.


## Mac UI and distribution (beta 2)

The [Mac experience specification](specs/0.3-mac-experience.md) supersedes the session-first initial layout. `DesktopControls` owns native application/menu bar commands; macOS capture accelerators dispatch through that boundary, without duplicate egui capture key handling. `Framecraft::capture` rechecks busy and active-session state. Shortcut preferences have a separate dialog draft; only successful Save changes committed bindings. Session interaction is disabled behind settings/replacement dialogs.

The app reads the installed SFNS font when available and falls back to its bundled UI fonts. It does not redistribute Apple's font. New captures/imports start without added styling; the CLI renderer's established default style remains unchanged.

Run `scripts/bundle-macos.sh`, `scripts/test-install-macos.sh`, then `scripts/package-macos.sh --skip-build`. Packaging verifies the bundle/version and writes a versioned DMG. `scripts/install-macos.sh` installs into Applications; updates require `--replace` and a stopped app. See [installation](INSTALLATION.md). App icon sources are in `packaging/make-icon.swift`; regenerate deliberately, never during an unrelated build. Preserve the macOS 13 deployment target and run native controls smoke after changing application menus.
