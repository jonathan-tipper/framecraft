# Framecraft 0.3.0-beta.1

A local Rust screenshot workbench for macOS. Collect frames from a fixed display region, reopen the session, explain selected images, and share a reviewed PNG or an offline evidence pack.

This is a **test build**, not a notarised release. macOS 13 is the deployment baseline; native capture, permission, shortcut, drag and accessibility checks across supported Macs are still outstanding. The bundle, session reopening, dark-image OCR and Cmd+Q edit protection were checked on macOS 26.6.2, Apple silicon, using synthetic images. See [test evidence](docs/TEST-NOTES.md) and the [test checklist](docs/TESTING.md).

## Try the build

You need a Mac, a current stable Rust toolchain, and Apple's command-line developer tools. From the repository checkout containing the 0.3 changes:

```sh
cargo run --locked --release

# Or build and open a local application bundle:
./scripts/bundle-macos.sh
open dist/Framecraft.app
```

The bundle uses identifier `dev.framecraft.app` and ad-hoc signing for local testing. It is not Developer ID signed or notarised. The [0.3.0-beta.1 prerelease](https://github.com/jonathan-tipper/framecraft/releases/tag/v0.3.0-beta.1) provides an Apple silicon bundle, checksum and synthetic evidence pack. Intel and clean-Mac distribution are not validated. To install your local build, quit any running copy and copy `dist/Framecraft.app` into your Applications folder.

macOS may request Screen Recording access for Framecraft, Terminal, or the IDE launching it. Grant access to the process macOS identifies and restart it if requested. Use synthetic content and a disposable destination for the first test.

## Capture, review, explain, share

1. In **Session**, open **New capture session**, choose a destination and display, then **Select shared area**. Drag a crop around the intended content and confirm **Use this area**; **Use full display** is also available.
2. Choose **On shortcut**, **On a timer**, or **When content changes**. Start the session. The app minimises; returning to it pauses collection.
3. Use the menu bar controls or `Cmd+Option+S` to capture. The default pause/resume shortcut is `Cmd+Option+P`; stop is `Cmd+Option+X`. Change bindings through **Shortcuts** while stopped.
4. Review the saved frames. Rename, reopen a session later, choose **Original** or a **Saved edited** variant, and open **Edit** to annotate.
5. Add arrows, rectangles, highlights, freehand ink, redactions, text labels or numbered steps. Select and revise marks; use undo/redo. **Save edited copy** preserves the original.
6. Copy the intended image, prepare it for native file drag, or select frames and choose **Review share pack…**. Review order, variants and captions before exporting the offline folder.

The capture region is fixed in the selected display's screenshot pixels. **It does not follow a window.** Stop and reselect after moving the content or changing displays. Size changes pause capture; swapping equal-sized displays is not detected.

| Mode | Behaviour |
| --- | --- |
| On shortcut | Capture now from the confirmed region while running |
| On a timer | 5–120-second interval, plus capture and processing time |
| When content changes | About one sample per second; saves sufficiently changed, settled pixels |

Default limits are 120 elapsed minutes, including pauses, or 500 images. Change detection does not understand slides: animation can prevent settling and small changes can be missed. Manual capture remains available in every mode. A [synthetic change-mode benchmark](docs/evidence/change-mode.md) saved 22 of 30 intended frames in each of 3 runs, missing small or low-contrast changes.

## What 0.3 adds

- Reopenable sessions with local versioned metadata, recent folders, original/edited variants and explicit import/recovery.
- Menu bar status and controls, configurable shortcuts, remembered capture defaults requiring source reconfirmation, and clearer source/destination failures.
- Selectable annotations, multiline text and numbered steps using the same pixel compositor as export.
- **Copy text…** from current rendered pixels using local Apple Vision OCR, with a text preview and explicit copy.
- Deliberate image handoff and selected offline HTML/PNG packs with neutral output filenames.

See the [release notes](docs/RELEASE-NOTES-0.3.md) for limits and the [product backlog](https://github.com/jonathan-tipper/framecraft/issues/1) for scope and evidence. User benefit has not yet been established by the [planned pilot](https://github.com/jonathan-tipper/framecraft/issues/9).

## Data and output boundaries

Original PNGs remain in the chosen session folder. **Redacting an edited copy does not remove the unredacted original.** Share the selected edited output or reviewed pack, not the whole session folder. Exported PNGs contain flattened pixels, without original layers or copied source metadata.

Session metadata remembers capture identity, names and saved variants. It does not save editable annotation layers or OCR transcripts. Quitting or replacing an edited document requires an explicit save/export, discard or cancel decision. Saving a PNG does not make its annotations editable on reopening.

Capture, editing and OCR make no application cloud calls. The capture adapter briefly writes a full-display image to a private temporary directory before cropping it. Prepared drag PNGs remain in temporary storage until the app exits. Cleanup is best effort, not secure erasure. Clipboard managers, cloud-synchronised destinations and receiving apps have their own retention behaviour.

Images use RGBA8. Colour-managed wide-gamut/HDR output is not implemented; imported colour profiles are discarded without a colour transform. Text labels use bundled Hack glyphs; unsupported characters show `?`, without complex-script shaping. OCR quality and supported languages depend on installed Apple Vision.

## Documentation and development

- [User guide](docs/USER-GUIDE.md): controls, recovery, sharing and troubleshooting.
- [Worked example](docs/WORKFLOW.md): a meeting capture through to an evidence pack.
- [Developer guide](docs/DEVELOPER-GUIDE.md) and [architecture](docs/ARCHITECTURE.md).
- [Testing](docs/TESTING.md), [recorded results](docs/TEST-NOTES.md), and [0.3 specification](docs/specs/0.3-session-workbench.md).
- [Product direction](docs/PRODUCT.md), [dependency inventory](docs/DEPENDENCIES.md), and [third-party notices](docs/THIRD-PARTY-NOTICES.md).

```sh
cargo run --locked --release -- --demo
cargo run --locked --release -- --demo-export /tmp/framecraft-demo.png
cargo run --locked --release -- --demo-session /tmp/framecraft-test-data
cargo run --locked --release -- --demo-pack /tmp/framecraft-test-data
cargo run --locked --release -- --render input.png output.png
```

`--demo-session` creates 10 synthetic originals and 10 edited variants in a new dated folder; open the printed folder through **Open session…**. `--demo-pack` creates `Framecraft-sample-pack` with 10 PNGs and an offline index inside the supplied parent directory, refusing an existing pack folder. Neither command captures the screen.

The UI and compositor are Rust, using egui/eframe rather than a webview. Original application code is MIT licensed. Apple capture and Vision are operating-system components. No Cap code is reused; Framecraft's proposed distinction is session collection and review, not being the first capture app to use Rust.
