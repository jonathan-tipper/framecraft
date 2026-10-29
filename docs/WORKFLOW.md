# Worked example: project review to evidence pack

Use a synthetic slide deck and a disposable folder for this exercise. It tests the complete workflow without requiring real meeting content.

1. In **Sessions → Start a new collection**, enter `Project review`, choose `~/Pictures/Framecraft`, and select the display showing the deck.
2. Choose **Choose capture area…**, crop around the slide content and confirm **Use this area**. Leave presenter notes and participant tiles outside the rectangle. Keep the content in that position.
3. Choose **When I click**, keep optional local heading naming enabled, and start. Framecraft minimises. Capture 3 distinct slides with `Cmd+Option+S` or the menu bar's **Capture now**.
4. Return to Framecraft to pause, then stop. Confirm all 3 originals are present. Rename one to `Delivery milestones` and check that a later OCR result does not replace your choice.
5. Edit the second frame. Add a text label, a numbered step and a redaction over a synthetic secret. Select and move a mark, then undo/redo. Inspect **Export preview** and save an edited copy.
6. Quit and reopen Framecraft. Use **Recent sessions** or **Open session folder…** to open the folder. Opening must not resume capture. The saved edited image is flattened; its earlier text and redaction objects are no longer editable.
7. Choose **Edited copy** for the second frame and **Original screenshot** deliberately for the others. Select only the first and second frames, reorder them and enter neutral captions.
8. Choose **Review share pack…**. Confirm 2 images plus `index.html`, check the redaction and captions, then export into a new folder outside the session.
9. Open the pack's `index.html` offline. Check that both PNGs resolve and that the third, unselected frame is absent. Share this output folder, not the source session.

Illustrative source layout, with UTC timestamps:

```text
~/Pictures/Framecraft/20261028-090000-project-review/
  framecraft-session.json
  .framecraft-session.lock
  20261028-090112-0001-manual-capture.png
  20261028-090130-0002-delivery-milestones.png
  20261028-090130-0002-delivery-milestones-edited-01.png
  20261028-090155-0003-operating-plan.png
```

Illustrative output layout:

```text
~/Desktop/Project-review-pack/
  index.html
  frame-0001.png
  frame-0002.png
```

The source folder preserves originals. The output uses neutral names and contains only selected flattened pixels and text you entered. A saved edited PNG is not styled again merely because you selected it for handoff.

For a one-image handoff, select the intended variant and choose **Copy image** or **More → Drag a copy…**. Keep Framecraft open until a native drag recipient has imported the PNG. For text extraction, open the frame in the Editor, choose **More Actions → Copy Text…**, review the current rendered result and explicitly copy it. OCR is not automatically copied into pack captions.

If you encounter a missing file, permission failure or metadata warning, follow the [recovery guide](USER-GUIDE.md#reopen-import-and-recover). Record the actual failure and build details in the [test checklist](TESTING.md); this example is an expected flow, not evidence that native checks have passed.
