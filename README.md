# Framecraft 0.3.0-beta.5 (development)

Capture an area of your Mac’s screen, annotate it, then copy or save the image. For meetings and presentations, collect a sequence of frames and return to it later. Images and OCR stay on your Mac.

This branch contains the next editor iteration. Beta 5 is not published yet; the downloadable beta 2 does not include these changes. Use the source build below to test this branch.

## Install on your Mac

Download the Apple silicon DMG from [the beta release](https://github.com/jonathan-tipper/framecraft/releases/tag/v0.3.0-beta.2). Open it, drag **Framecraft** onto **Applications**, then launch Framecraft from Applications. The capture-frame icon appears in your menu bar; Framecraft also has a Dock icon and normal application menus.

This beta is ad-hoc signed, not Developer ID signed or notarised. macOS may block the first launch. Follow the [installation guide](docs/INSTALLATION.md) and only allow this specific build if you trust it. macOS 13 is the deployment baseline; Intel, older-system runtime and clean-Mac distribution remain unverified. See [recorded checks](docs/TEST-NOTES.md).

## Take your first screenshot

1. On first launch, use **Capture setup** to choose a global shortcut, save folder, filename prefix, optional UTC date folders and whether to edit or save automatically. **Not now** skips setup. You can reopen it at any time.
2. Click **Capture Area…** in the window or from the Framecraft menu bar icon.
3. Drag a rectangle around the content on screen. Press Escape to cancel.
4. Review the result. Use **Copy Image**, **Save PNG…**, or an annotation tool. **Background & framing** adds optional presentation styling; new captures start with unchanged dimensions.

**Capture Window…** selects a window. **Capture Screen** takes the primary display. **Open Image…** imports a PNG or JPEG. The same capture commands are in the Mac’s **Capture** application menu. The app requests the screen access macOS requires; it never grants itself permission.

Shortcuts are optional. **Command (⌘)** and **Option (⌥)** are Mac modifier keys. While Framecraft is active, **Command + Option + 4** captures an area, **5** a window and **3** the primary display. Use **Capture setup** for an everyday shortcut that works from other apps while Framecraft is running. **More Capture → Session & app shortcuts…** contains the existing app commands and session bindings. You do not need to type key syntax.

## Edit and leave a screenshot

The development editor uses a labelled tool rail and a contextual inspector. Choose **Highlight** for named colour presets and custom RGB, or **Pixelate** to draw an area and adjust **Block size**. The inspector distinguishes settings for the next mark from properties of a selected mark. Use **Redact** for an opaque cover; pixelation is not guaranteed concealment.

**Text** now works in placement order: choose Text, click the image, type, then **Add text**. Select a label and choose **Edit text…**, or double-click it, to revise it. **Apply changes** commits one undo step; Cancel/Escape preserves the original.

Automatic saving is optional. It saves the captured pixels, then opens them for editing; subsequent edits are saved separately. If automatic saving fails, the capture stays open for manual saving. Reopened PNGs can receive new marks, but their previous text is flattened. Persistent editable documents are tracked in [#33](https://github.com/jonathan-tipper/framecraft/issues/33).

**Cancel tool** returns to Select and preserves completed marks. Use Undo after committing a mark. **Close screenshot** leaves Framecraft running; a fresh capture or unsaved edit offers Save, Discard changes and Cancel. Copying to the clipboard does not count as saving a file. Native checks and outstanding limits are recorded in [TEST-NOTES](docs/TEST-NOTES.md).

## Collect several frames

Open **Sessions → Start a new collection**. Choose the capture area, save folder and collection mode, then **Start collection**. Session-area selection uses a display snapshot: draw the crop, then confirm **Use this area**. Returning to Framecraft pauses collection. Stop the collection before using a separate one-off capture.

| Mode | Behaviour |
| --- | --- |
| When I click | Use Capture now or Command + Option + S while running |
| On a timer | 5–120-second interval, plus capture and processing time |
| When content changes | Samples about once per second and saves sufficiently changed, settled pixels |

Session shortcuts work from other apps only during an active collection. Defaults are Command + Option + S to capture, P to pause/resume and X to stop. **Collection options** contains timing, naming and stop limits. Default limits are 120 elapsed minutes, including pauses, or 500 images.

The repeated capture region is fixed. **It does not follow a window.** Stop and reselect after moving content or changing displays. Equal-sized display swaps are not detected. The [synthetic change-mode benchmark](docs/evidence/change-mode.md) saved 22 of 30 intended frames in each of 3 runs; small or low-contrast changes can be missed.

Reopen a saved session, choose an original or edited copy, add arrows/text/redaction, copy an image or export selected frames through **Review share pack…**. Packs contain offline HTML and flattened PNGs. Use **More Actions → Copy Text…** in the screenshot editor for local OCR with explicit review and copy.

## Try the build

To build from source, use a Mac with stable Rust and Apple’s command-line developer tools:

```sh
./scripts/bundle-macos.sh
./scripts/install-macos.sh
# For an existing install, quit Framecraft and use:
./scripts/install-macos.sh --replace
# Create a distributable local DMG:
./scripts/package-macos.sh
```

No Rust tools are needed to use the downloadable app. See the [developer guide](docs/DEVELOPER-GUIDE.md) for source setup and the [release notes](docs/RELEASE-NOTES-0.3.md) for changes and limits.

## Data and output boundaries

Original PNGs remain in the chosen session folder. **Redacting an edited copy does not remove the unredacted original.** Share the selected edited output or reviewed pack, not the whole session folder. Exported PNGs contain flattened pixels, without original layers or copied source metadata.

Session metadata remembers capture identity, names and saved variants. It does not save editable annotation layers or OCR transcripts. Quitting or replacing an edited document requires an explicit save/export, discard or cancel decision. Saving a PNG does not make its annotations editable on reopening.

Capture, editing and OCR make no application cloud calls. The capture adapter briefly writes a full-display image to a private temporary directory before cropping it. Prepared drag PNGs remain in temporary storage until the app exits. Cleanup is best effort, not secure erasure. Clipboard managers, cloud-synchronised destinations and receiving apps have their own retention behaviour.

Images use RGBA8. Colour-managed wide-gamut/HDR output is not implemented; imported colour profiles are discarded without a colour transform. Text labels use bundled Hack glyphs; unsupported characters show `?`, without complex-script shaping. OCR quality and supported languages depend on installed Apple Vision.

## Documentation and development

- [Installation](docs/INSTALLATION.md) and [Mac experience specification](docs/specs/0.3-mac-experience.md).
- [User guide](docs/USER-GUIDE.md): controls, recovery, sharing and troubleshooting.
- [Worked example](docs/WORKFLOW.md): a meeting capture through to an evidence pack.
- [Developer guide](docs/DEVELOPER-GUIDE.md) and [architecture](docs/ARCHITECTURE.md).
- [Testing](docs/TESTING.md), [recorded results](docs/TEST-NOTES.md), and [0.3 specification](docs/specs/0.3-session-workbench.md).
- [Product direction](docs/PRODUCT.md), [dependency inventory](docs/DEPENDENCIES.md), and [third-party notices](docs/THIRD-PARTY-NOTICES.md).
- [Editor design prototype](docs/prototypes/editor/README.md): proposed UI, clickable flows and story mapping for #22–#28. This is a design candidate, not a shipped app feature.

```sh
cargo run --locked --release -- --demo
cargo run --locked --release -- --demo-export /tmp/framecraft-demo.png
cargo run --locked --release -- --demo-session /tmp/framecraft-test-data
cargo run --locked --release -- --demo-pack /tmp/framecraft-test-data
cargo run --locked --release -- --render input.png output.png
```

`--demo-session` creates 10 synthetic originals and 10 edited variants in a new dated folder; open the printed folder through **Open session folder…**. `--demo-pack` creates `Framecraft-sample-pack` with 10 PNGs and an offline index inside the supplied parent directory, refusing an existing pack folder. Neither command captures the screen.

The UI and compositor are Rust, using egui/eframe rather than a webview. Original application code is MIT licensed. Apple capture and Vision are operating-system components. No Cap code is reused; Framecraft's proposed distinction is session collection and review, not being the first capture app to use Rust.
