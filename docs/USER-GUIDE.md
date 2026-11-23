# Framecraft 0.3 user guide

This guide covers beta 6. For a test report, include the version and running-copy path shown in **More Capture → Screen Access… → Which copy is running?**. [Installation guide](INSTALLATION.md) · [Test results](TEST-NOTES.md)

## Install and take a screenshot

Drag Framecraft from the disk image into Applications, eject it and open the installed copy. The [installation guide](INSTALLATION.md) explains the signing status of each build and how to approve Screen Recording.

Choose **Capture Area…** in the window or from the menu bar icon. Drag a rectangle. Escape cancels. In the default editor mode, the selected pixels open for annotation. For capture from another app, configure **Copy and return** or **Save and return** under [Set up everyday capture](#set-up-everyday-capture). Stop an active collection before taking a one-off grab.

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

The capture-frame menu bar icon shows **Capture Area** with the everyday shortcut status. It also offers window and screen capture and separate session controls. After a direct grab, **Edit Last Capture…** opens the retained image. **Clear Last Capture** releases it. A failed output also offers **Retry Last Output**. Capture actions appear in the native **Capture** application menu.

| Default binding | Action |
| --- | --- |
| `Cmd+Option+S` | Capture selected region while running |
| `Cmd+Option+P` | Pause/resume an active session |
| `Cmd+Option+X` | Stop an active session |
| `Cmd+Option+4`, `5`, `3` | One-off area, window, primary-screen capture while Framecraft is active |
| `Cmd+,` | Open Capture setup |
| `Cmd+O` | Open a PNG/JPEG in the Editor |
| `Cmd+S` | Save edited session copy, or export a standalone PNG |
| `Cmd+Shift+C` | Copy current composed image in the Editor |
| `Cmd+Z`, `Cmd+Shift+Z` | Undo, redo |

Capture commands are application-wide while Framecraft is active and no collection is running. Save/copy/undo apply to the visible Screenshot editor; they do not edit a hidden document in Sessions. Text-entry controls take priority over document editing shortcuts.

Open **More Capture → Session & app shortcuts…** for the session bindings and application command reference. Command (⌘) and Option (⌥) are the Mac keys beside the space bar. The first section lists screenshot commands. The second has checkboxes for Command, Option, Control and Shift, plus a key menu for the three session actions. Choose at least one modifier. Letters, digits and F1–F24 are available. Duplicate or reserved bindings show an error; if another app occupies a combination, choose another. Existing saved shortcuts keep their meaning. **Restore Defaults** changes the form; **Save Shortcuts** keeps the changes. Session shortcuts become active only when a collection starts.

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

The left rail names each tool alongside its icon; the right inspector shows the relevant properties. Use arrows, rectangles, highlights, pixelation, freehand ink, redactions, text or numbered steps. **Select** chooses a mark for revision, movement or deletion. When objects overlap, selection follows their visual order: redactions take priority, ordinary annotations sit above pixelation, and later pixelation regions cover earlier ones. Use **Select mark** to choose an obscured mark. **Delete selected mark** removes it; the Left/Right/Up/Down controls nudge it by 1 source pixel. Moving keeps a shape together and within the source bounds; an oversized label must be reduced before it can move.

Choose **Text**, click the image where the label should start, then type into the focused **Add text** dialog. Choose **Add text** to commit, or **Cancel** / Escape to discard the draft. Select an existing label and choose **Edit text…**, or double-click it, to change wording, size and colour together. **Apply changes** creates one undo step. Empty labels cannot be committed. Text supports explicit newlines, up to 2,048 Unicode characters and 32 lines, at 8–128 source pixels. The embedded Hack font displays unsupported glyphs as `?`; there is no automatic wrapping or complex-script shaping. Numbered steps have separate controls, values 1–9,999 and a badge-size setting. Preview and export share the pixel renderer. Undo/redo affects in-memory edits.

Opaque redactions render after other marks and destroy the covered pixels in the flattened export. The source image and undo history remain in memory, and the original file remains on disk. **Save edited copy** writes another PNG beside the original; a normal PNG export is also available.

**Close screenshot** leaves the current editor without quitting Framecraft. A new capture or an image with unsaved edits asks for **Save**, **Discard changes** or **Cancel**. Copying the image does not save a file and does not remove this decision. Cancel or a failed/cancelled save keeps the screenshot and edits. A clean file-backed image can close directly. Closing a session image returns to session review; closing a standalone image returns to capture-ready. Discard removes in-memory work, not original or previously saved files. Quit and replacement retain their separate save guards. Saved PNGs are flattened: reopening does not restore text objects, redaction objects or undo history. There is no editable-project format in 0.3.

## Highlight, pixelate and correct a mistake

Choose **Highlight**, then a named colour in **Next highlight**, and draw a rectangle. Select an existing highlight to see **Selected highlight** and recolour only that mark. Next-tool defaults remain separate. **Custom colour…** provides RGB selection with **Apply colour** and **Cancel**; highlights stay translucent. There is no opacity or stroke-weight setting for a rectangular highlight.

Choose **Pixelate** and draw over the detail to obscure. **Block size** is measured in the original image's pixels, from 2 to 128; the starting value is 12. Larger values create coarser blocks. Select a completed region to adjust its size setting, move it or delete it. Changing **Next pixelation** affects the next region only. Use Undo/Redo for committed changes. Pixelation does not offer region resizing in this increment.

Pixelation reduces detail; use **Redact** for an opaque cover. Pixelation sits below ordinary annotations, and Redact renders last. The original file and undo history remain available, so share the flattened output rather than the original when concealment matters.

**Cancel tool** returns to Select while retaining completed marks. During a held drawing, crop or movement gesture, press Escape before releasing to abandon that gesture. After release, use Undo instead. A dialog's Cancel applies to that dialog, not the screenshot or collection.

For one-off capture, **Capture → Cancel Capture** is enabled while the capture operation is active. Its pointer reachability while the system selector owns input has not yet been established; Escape remains the native selector's cancellation route. Actual selector cancellation and focus restoration must be checked on the test Mac. The application deliberately hides capture controls to avoid including them in the screenshot.

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


## Set up everyday capture

First launch opens **Make capture yours**. Reopen it from **Capture setup** or the app’s Settings menu. **More Capture → Session & app shortcuts…** contains the separate session controls.

1. **Access.** Check screen permission and request it when ready. Finish setup even if you want to defer capture access. macOS controls the grant.
2. **Shortcut.** Choose **Record shortcut**, press the keys together, and confirm your choice. Escape abandons recording. The alternative controls let you select individual keys. **Use menu bar only** keeps capture available without a global shortcut. The suggested ⌘⇧2 means Command–Shift–2. If another app already uses it, choose another chord. Ordinary typing, standard edit commands and Apple’s screenshot shortcuts are protected.
3. **Files.** Choose a folder, filename prefix and optional UTC date folders. The preview shows where a saved grab will go. Two grabs in the same second get distinct names.
4. **Finish.** Choose the result you want in the table below, then finish setup. The installed shortcut works while Framecraft is running. Try it from another app and check the active binding under the menu bar icon. Cancelling setup keeps your previous choices.

| After capture | What happens |
| --- | --- |
| Open the editor | Review the pixels and choose whether to copy or save. This is the initial default. |
| Save automatically, then open for optional editing | Save the original PNG and open it in the editor. Existing beta 5 preferences keep this behaviour. |
| Copy and return | Put the region on this Mac’s clipboard. Press ⌘V at the original insertion point in an app that accepts images. |
| Save and return | Write the original PNG to your chosen folder, retain your clipboard and continue in the source app. |

After Finish, **Try your shortcut** explains how to test it in another app. macOS can report an exclusive registration conflict. Some utilities use shared shortcuts that macOS accepts for both apps. If a shortcut changes the other utility’s behaviour, choose a different combination or **Use menu bar only**.

Both direct modes offer an optional thumbnail. If Framecraft cannot identify the capture display, use **Edit Last Capture…** in the menu bar. This is the normal route for an area or window grab with multiple displays. Opening the retained image can prompt about an unsaved edit.

Framecraft holds one direct capture in memory. **Last Capture Options → Clear Last Capture** or quitting releases it. Files and content already copied to the pasteboard have their own lifetimes. If direct output fails, the menu bar icon shows **Copy failed** or **Save failed**. Choose **Last Capture Options → Retry Last Output** or **Edit Last Capture…** to recover it. For a failed folder write, repair the destination or open the image and use **Save PNG…**.

**Copy and return** requests a host-only macOS pasteboard. It excludes that write from Universal Clipboard. An explicit copy from the editor uses the ordinary system clipboard, which can participate in Apple’s Universal Clipboard if enabled. Local clipboard managers and receiving apps can retain their own copies. [Apple Universal Clipboard](https://support.apple.com/en-gb/102430)

Preferences live at `~/Library/Application Support/Framecraft/quick-capture.json`. Reopen Capture setup to change them. A legacy shortcut that could intercept typing or editing needs review before it can be registered. Framecraft also works from its menu bar while keys are off.

For later editing, use Open Image or drop a PNG/JPEG into the app. Previous exported annotations are flattened pixels; an editable document format is planned under [#33](https://github.com/jonathan-tipper/framecraft/issues/33).
