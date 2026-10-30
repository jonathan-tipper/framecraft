# Framecraft 0.3 user guide

This guide describes the 0.3.0-beta.2 test build. Start with synthetic content. The native test matrix remains incomplete; [test notes on GitHub](https://github.com/jonathan-tipper/framecraft/blob/v0.3.0-beta.2/docs/TEST-NOTES.md) record actual results. This link also works when reading the guide bundled with the app.

## Install and take a screenshot

Install from the DMG by dragging Framecraft onto Applications, then open it there. This is an ad-hoc signed beta; see the [installation guide on GitHub](https://github.com/jonathan-tipper/framecraft/blob/v0.3.0-beta.2/docs/INSTALLATION.md) for first-launch and update instructions.

Click **Capture Area…** in the window or in the capture-frame menu bar icon. Drag directly on screen; Escape cancels. The window returns with the selected pixels ready for **Copy Image** or **Save PNG…**. **Capture Window…** and **Capture Screen** are alternatives. Capture is unavailable while a collection is active; stop it first. Unsaved edits prompt before replacement.

**Screenshot** is the editor. New captures and imports retain their pixel dimensions and have no added background or shadow. **Background & framing** is optional. **More Actions** contains **Copy Text…**, **Drag Image File…** and, for saved images, **Reveal Saved Image in Finder**.

## Set up a capture session

Open **Sessions → Start a new collection**. Enter a title, choose a destination and set the display number. Framecraft creates a new dated subfolder inside that destination. Use **Choose capture area…** to take a display snapshot, drag a crop around the useful content, then confirm **Use this area**. Confirming **Use full display** includes everything on that display.

Keep participant tiles, chat and other unwanted content outside the rectangle. The source is a fixed display rectangle, not a tracked window. It stays fixed when content moves. A source dimension change pauses capture; equal-sized display reordering cannot be detected. Stop and select again after changing your display arrangement.

Choose a collection mode:

| Mode | Suitable use | Limit |
| --- | --- | --- |
| When I click | Deliberately collect particular frames | Requires a running session and a manual trigger |
| On a timer | Collect regular samples | 5–120 seconds; processing adds time |
| When content changes | Collect settled presentations | Pixel-based detector; animation and small edits can defeat it |

**Collection options** contains changed-area and settling controls, naming and stop limits. These are tuning controls, not a guarantee of one capture per slide. **Capture now** works in all running modes. Default stop limits are 120 elapsed minutes, including pauses, or 500 captures; the UI allows up to 480 minutes or 2,000 captures.

Start minimises Framecraft. Returning to Framecraft pauses collection. Resume minimises it again and resets change detection, so a previously saved stable frame may be captured again. Pause and Stop invalidate pending work; a file save already underway may finish. Source and setup controls are disabled while running or paused. Stop before changing them.

## Permissions, controls and shortcuts

Grant Screen Recording access to the app identified by macOS. Depending on how you launched the build, this can be Framecraft, Terminal or an IDE. Open **System Settings → Privacy & Security** if access was denied, change the relevant screen-recording permission, then restart the named app if requested. Labels differ by macOS version.

The capture-frame menu bar icon offers **Capture Area…**, **Capture Window…**, **Capture Screen**, **Open Framecraft**, **Keyboard Shortcuts…** and **Quit Framecraft**. Separate session commands report collection state and saved count. Capture actions also appear in the native **Capture** application menu. The controls attempt to hide during capture; verify this on the Mac you use.

| Default binding | Action |
| --- | --- |
| `Cmd+Option+S` | Capture selected region while running |
| `Cmd+Option+P` | Pause/resume an active session |
| `Cmd+Option+X` | Stop an active session |
| `Cmd+Option+4`, `5`, `3` | One-off area, window, primary-screen capture while Framecraft is active |
| `Cmd+O` | Open a PNG/JPEG in the Editor |
| `Cmd+S` | Save edited session copy, or export a standalone PNG |
| `Cmd+Shift+C` | Copy current composed image in the Editor |
| `Cmd+Z`, `Cmd+Shift+Z` | Undo, redo |

Capture commands are application-wide while Framecraft is active and no collection is running. Save/copy/undo apply to the visible Screenshot editor; they do not edit a hidden document in Sessions. Text-entry controls take priority over document editing shortcuts.

Open **Keyboard Shortcuts…** from the window or menu bar. Command (⌘) and Option (⌥) are the Mac keys beside the space bar. The first section lists screenshot commands. The second has checkboxes for Command, Option, Control and Shift, plus a key menu for the three session actions. Choose at least one modifier. Letters, digits and F1–F24 are available. Duplicate or reserved bindings show an error; if another app occupies a combination, choose another. Existing saved shortcuts keep their meaning. **Restore Defaults** changes the form; **Save Shortcuts** keeps the changes. Session shortcuts become active only when a collection starts.

In **Collection options**, **Save as defaults** retains the title, destination, display number, mode, timing/change controls, naming choice and stop limits. Defaults are also saved when starting or closing the app. **Restore defaults** restores the initial settings. A previous region is shown only as a hint: select and confirm the source again before capture. Restart never resumes a session automatically.

## Names, originals and saved edits

An original is saved before optional naming starts. Names contain a UTC timestamp, sequence and sanitised title, for example `20261028-090112-0001-delivery-milestones.png`. Heading naming uses local Apple Vision OCR plus deterministic selection rules. It does not generate a description or summary.

OCR failure, a full naming queue or text with no useful heading leaves a fallback name. The PNG is still saved. **Rename** is authoritative even if a background suggestion arrives later. Collisions receive a new suffix instead of replacing another capture.

In **Collected screenshots**, choose **Original screenshot** or an **Edited copy** variant before copying, dragging, revealing, editing or adding it to a pack. Saved variants load their already flattened pixels without automatically adding a second border or shadow. A missing edited file causes an error; the app does not silently substitute its original. **Reveal** locates the selected saved file in Finder. It does not save current editor changes.

## Reopen, import and recover

Choose **Open session folder…** and select the session folder, or use **Recent sessions**. Opening is for review; it does not start capture. **Forget** removes a recent-folder reference, not the folder or images. If a folder moved, locate it again through **Open session folder…**.

New sessions contain `framecraft-session.json`, schema version 1. It stores stable session/capture IDs, titles, sequence numbers and relative PNG records. Keep this file beside the PNGs when moving or backing up a session. A per-folder lock prevents simultaneous writers; another open copy or an incompatible filesystem can prevent opening.

For a legacy folder, the app previews recognised capture files before **Confirm and open** creates metadata. Recognition expects Framecraft's timestamp/sequence PNG naming pattern. Arbitrary PNG names, symbolic links, duplicate sequences and unattached `-edited-NN.png` files are not automatically imported. Open those files individually in the Editor if needed. Cancelling the preview leaves the folder unchanged.

| Problem | Recovery |
| --- | --- |
| Interrupted save left a recognised PNG outside metadata | Reopen, inspect the import preview and confirm recovery |
| Invalid version-1 metadata | Review recovery; confirmation preserves a recovery JSON backup before rebuilding from recognised PNGs |
| A newer/unknown schema version | Use a compatible Framecraft version; no automatic downgrade occurs |
| Missing original | Use **Import replacement original…**; the selected PNG is copied into the session |
| Original renamed externally | Byte-identical recognised files can be relinked after confirmation |
| File modified externally or unreadable | Review the error and explicitly import a replacement; modified bytes are not silently trusted |
| Missing edited variant | Restore the saved PNG from backup or choose another available variant explicitly |
| Session is locked | Close the other process; if filesystem locking is unsupported, copy the complete folder to a local Mac disk |

Recovery cannot recreate lost image pixels or lost annotation layers. Keep a backup before deliberately testing corruption or interruption.

## Explain a frame

Choose **Edit image** on a frame, import a PNG/JPEG, or use **Capture Area…**. All drawing uses source-image coordinates; the crop and presentation styling are applied afterwards.

Use arrows, rectangles, highlights, freehand ink, redactions, text or numbered steps. **Select** chooses a mark for revision, movement or deletion. When objects overlap, the visible top mark wins; redactions take priority because they render last. Use **Select mark** to choose an obscured mark. **Delete selected mark** removes it; the Left/Right/Up/Down controls nudge it by 1 source pixel. Moving keeps a shape together and within the source bounds; an oversized label must be reduced before it can move.

Text labels support explicit newlines, up to 2,048 Unicode characters and 32 lines. The **Size** slider offers 8–128 pixels. They use an embedded Hack font. Unsupported glyphs appear as `?`; there is no automatic wrapping or complex-script shaping. Numbered steps support 1–9,999; **Size** changes the badge radius. Choose the tool and click the image to place a label or step, or use **Apply label / step properties** to revise the selected object. Preview and export share the pixel renderer. Undo/redo affects in-memory edits.

Opaque redactions render after other marks and destroy the covered pixels in the flattened export. The source image and undo history remain in memory, and the original file remains on disk. **Save edited copy** writes another PNG beside the original; a normal PNG export is also available.

Quitting or replacing a document with unsaved edits asks for save/export, discard or cancellation. An export failure keeps the edits available. Saved PNGs are flattened: reopening does not restore text objects, redaction objects or undo history. There is no editable-project format in 0.3.

## Copy recognised text

Use **More Actions → Copy Text…** in the Screenshot editor. Recognition receives the current composed image, including crop, styling and redactions. Review the resulting text, then explicitly copy it. Editing or replacing the image invalidates an old result. Failure, cancellation and empty results leave the clipboard unchanged.

This is local Apple Vision OCR. Numbers, URLs, interface labels and footer text are retained, unlike filename heading selection. Reading order is approximate: top-to-bottom rows and left-to-right regions. Tables, columns and complex layouts can need correction. Supported languages and recognition quality depend on installed Apple Vision.

Recognition composites transparent pixels onto white and normalises predominantly dark images in a temporary working buffer. The displayed/exported image is unchanged. Input is reduced to at most 4,096 pixels on its longest axis. Results are bounded to 512 regions and 32,768 Unicode characters; exceeding a result limit produces an error rather than a truncated transcript. Crop to a smaller area if necessary, including when a complex scene misses otherwise legible text. OCR transcripts are not written into session metadata or image files.

## Copy, drag or export a pack

**Copy image** uses the chosen saved variant in Session, or **Current edits** in the Editor. **More → Drag a copy…** in Sessions, or **More Actions → Drag Image File…** in Screenshot, creates a flattened PNG named `frame-0001.png`; drag its prepared-file control into an accepting application. Keep Framecraft open until the receiver has imported it. Prepared files stay in private temporary directories until exit, with best-effort cleanup. Cancelling the native drag does not remove the prepared file immediately. Receiving-app support requires native testing; use copy or PNG export if a target rejects the drag.

For multiple frames:

1. Select the frames in **Collected screenshots** and choose each saved variant.
2. Enter captions and use **Move up/Move down** to set order.
3. Choose **Review share pack…**. Check every preview, variant, caption and the file count.
4. Choose a new folder name and a destination outside the source session folder and its descendants. Export.
5. Open the completed `index.html` locally. Share the whole output folder so its relative image links remain valid.

A pack contains only `index.html` plus one flattened PNG per selected frame, named `frame-0001.png`, `frame-0002.png` and so on. It needs no Framecraft installation, scripts, hosted service or external assets. Titles start neutral and captions blank; source titles, filenames, paths and OCR text are not added automatically. Explicitly entered captions are escaped as text.

The initial limit is 100 frames and 64 million combined pixels; captions allow 2,000 characters and the pack title 200. Split a large pack if it exceeds the bound. Export stages all files before publishing a new folder and does not overwrite an existing folder. Failure or cancellation preserves the review draft. Unsaved **Current edits** must be saved as an edited copy before selecting that variant for a session pack.

## Troubleshooting and storage

If capture pauses, read the error before resuming. Check screen access, the confirmed display/region, available disk space and write access to the chosen destination. A failed destination probe creates no session. A hanging unattended capture has a deadline; interactive area/window selection waits for you to complete or cancel it.

If a PNG was saved but metadata failed, keep the session folder and reopen it for recovery. Do not delete the acknowledged image to silence the warning. If recent-session or shortcut preferences are corrupt, the app reports the problem or falls back to defaults; inspect a backup before replacing files under `~/Library/Application Support/Framecraft/`.

| Location | Contents and lifetime |
| --- | --- |
| Chosen session folder | Original PNGs, saved edited PNGs, manifest, lock file and any recovery backups; retained until you delete them |
| `~/Library/Application Support/Framecraft/recent-sessions.json` | Up to 12 recent-folder references |
| `~/Library/Application Support/Framecraft/shortcuts.json` | Session shortcut preferences |
| `~/Library/Application Support/Framecraft/capture-preferences.json` | Stopped capture defaults and a source-region hint; no running target |
| Private system temporary directories | Capture intermediates and prepared drag PNGs; cleanup is best effort |
| Clipboard | Last deliberately copied image/text; retention depends on macOS and other apps |

Framecraft does not automatically delete originals or clean up session folders. Cloud-synchronised destination folders can upload your files through another application. The current renderer uses RGBA8 and does not provide colour-managed HDR/wide-gamut output.
