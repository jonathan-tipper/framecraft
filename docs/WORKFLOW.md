# A capture session, from setup to sharing

Example: collect slides during a project review, while leaving participant tiles and meeting chat out of the saved region.

1. Open **Session**. Enter `Project review`, choose a destination such as `~/Pictures/Framecraft`, and choose the display showing the shared content.
2. Click **Select shared area**. In the display snapshot, use **Crop** to mark the slides, then click **Use this area**. This fixes a rectangle in screenshot pixels. Keep the meeting window and shared content in that position.
3. Choose **On shortcut** for deliberate captures, **On a timer** for regular samples, or **When content changes** for settled visual changes. Leave local OCR naming enabled if headings are useful.
4. Press **Start session**. The app minimises. Use `Cmd+Option+S` to save a frame without another selector or save dialog. This manual capture also works in timer and change modes.
5. Use `Cmd+Option+P` to pause or resume and `Cmd+Option+X` to stop. Opening Framecraft pauses capture for review. Resume minimises it again.
6. In **Collected frames**, correct a title and click **Rename**, or choose **Edit**. Highlight, draw, crop, or redact, then press **Save edited copy** or `Cmd+S`.

Illustrative paths, with UTC timestamps:

```text
~/Pictures/Framecraft/20261019-090000-project-review/
  20261019-090112-0001-manual-capture.png
```

The PNG is already saved at this point. If OCR later finds `Delivery milestones`, the same capture can become:

```text
20261019-090112-0001-delivery-milestones.png
```

An edited export is another file in that folder:

```text
20261019-090112-0001-delivery-milestones-edited-01.png
```

These names are examples, not files included with the application. Timestamp and sequence stay intact during renaming. A collision receives a suffix rather than replacing an existing image.

## Things to account for

- **Moving content:** the region does not follow a window. Stop and reselect after moving or resizing the meeting content. A display-size change pauses capture, but an equal-sized display taking another index is not detected.
- **Change mode:** the detector recognises pixel changes, not slide meaning. Animations can keep it unsettled, and small changes can fall below the threshold. Use the manual shortcut for a frame you particularly need.
- **Timing:** sampling and OCR are asynchronous. The settling control is evaluated on sample ticks, not a precise subsecond trigger.
- **Pausing and resuming:** Resume resets change detection, so the last stable frame can appear again. Pause or Stop discards in-flight captures unless atomic file persistence has already begun; that save can finish. A shortcut-registration conflict leaves the session paused until it can be resolved and registration retried.
- **Naming:** diagrams, sparse text, failed OCR, or a busy naming queue can keep fallback names. The PNG is still saved. A manual rename takes priority over pending OCR, and opening a frame for editing is synchronised with background filename changes. OCR text is not retained as a sidecar.
- **Stopping:** the default stop limit is 120 elapsed minutes, including pauses, or 500 images. Stopping retains saved files. To change the source region or session settings, stop first.
- **Redaction:** edited copies contain flattened redactions, but the unredacted originals remain beside them. Share the intended edited files, rather than the whole session folder.

Session setup and the thumbnail list are currently in-memory application state. Saved PNGs survive quitting; automatic session reopening and editable project files are not implemented. You can reopen a PNG through the editor.
