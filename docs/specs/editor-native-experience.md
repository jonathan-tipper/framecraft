# Native editor experience

## Scope and status

This is the implementation contract for [epic #22](https://github.com/jonathan-tipper/framecraft/issues/22) and stories #23–#28. It adopts the [editor prototype design](editor-experience-prototype.md) for the Rust/egui Mac app. The issue bodies own product acceptance; this document records implementation decisions and test obligations. The browser prototype is a reference, not evidence of native acceptance.

This is development work after beta 2. Results belong in [TEST-NOTES](../TEST-NOTES.md); pending checks do not become passes when their controls appear. No framework, macOS baseline, persistence schema or capture-target migration is included.

## Product flow

Capture or open → choose a tool → draw/adjust → copy or save → Close screenshot → capture-ready or the originating session. Users can recover a mistake without quitting Framecraft. Sessions remain a separate route and retain their existing collection state authority.

The shell uses warm neutral surfaces, charcoal text, a cobalt accent, a labelled 132-point tool rail, a central image and a right-hand contextual inspector. Existing annotation tools remain available. Icons use original vector primitives alongside words; they do not introduce dependencies, hidden command routes or new shortcuts. Supporting native menus remain available.

## Lifecycle and cancellation

| State/action | Contract |
| --- | --- |
| Idle selection | No active-operation Cancel is implied. Selection changes do not alter image pixels. |
| Drawing tool active | Cancel tool returns to Select and retains committed marks. |
| Held draw/freehand/crop/move | Escape restores the starting state without adding an abandoned edit to Undo. Release commits; Undo reverses the committed action. |
| Nested property/dialog | The nearest cancellable layer owns Escape. Cancel retains the document and restores its opening value; it must not trigger Close or Quit. |
| Fresh capture | It remains unsaved even without annotations and after Copy. |
| Close screenshot | Dirty images and fresh captures require Save / Discard changes / Cancel. A clean file-backed image closes directly. |
| Save cancelled or failed | Retain the document and all edits. Report failure only when a save failed. |
| Save completed | Acknowledge only the document/revision that produced the durable output. Stale completion cannot close newer work. |
| Close completed | Clear editor state and return to capture-ready or session review. Do not delete originals, saved variants or session records. Framecraft remains open. |
| Quit and Stop collection | Remain separate existing commands and guards. |

The ordinary capture adapter accepts a cancellation callback. Before native selection, cancellation returns without capturing. During selection it terminates and reaps the child; after decoding it checks again before handing pixels to the editor. Temporary capture files remain private and are removed through existing RAII cleanup. Cancellation is idempotent. A cancelled one-off capture returns no replacement image; repeated-session capture retains its separate error/state contract.

The application and tray menus include a gated **Cancel Capture** action for the active one-off operation. The existing capture-control visibility guard is retained. **Pointer accessibility of that menu while macOS owns selection remains a native feasibility check**: the tray is hidden during capture and the system selector may consume menu interaction. Do not claim C4 from the menu implementation alone. Escape, focus/minimisation restoration and capture exclusion also need native evidence.

## Property targets and edit transactions

**Next highlight / Next pixelation** changes defaults for the next mark; it does not dirty the document. **Selected highlight / Selected pixelation** reads and changes that mark only; defaults and other marks remain unchanged.

Highlight offers Yellow, Green, Cyan, Blue, Pink and Orange plus custom RGB. It retains renderer alpha 80/255; there is no ineffective alpha or Weight control. Custom colour must expose an explicit Apply/Cancel boundary. The accepted design calls for live preview, rollback and useful focus return; test those independently rather than inferring them from a colour picker. One completed selected-property adjustment is one undo unit.

Pixelation uses **Block size**, 2–128 original-image pixels, default 12. Larger blocks obscure more detail. Selection, moving, deleting and undo/redo use the existing source-coordinate annotation model. Continuous adjustments require a starting snapshot, immediate preview, one commit and Escape rollback. Typed invalid values must not reach rendering. Resizing pixelation and adjustable highlight opacity remain outside the first required slice.

## Pixelation compositor contract

`Annotation::Pixelate { rect, block_size }` is a source-space rectangle. The shared compositor supplies editor output, PNG, clipboard, prepared drag, OCR input and saved copies; no separate export-only effect is permitted.

1. Validate image and crop bounds, block size and the image-clipped region. A usable region must cover at least 2 × 2 source pixels.
2. Use a stable source buffer with active opaque redactions applied before sampling. Pixels outside the crop cannot contribute to any visible block average.
3. Anchor the block grid at the pixelation region's top-left. Each sample is the block intersected with region, source image and crop. Edge blocks average only their eligible pixels.
4. Compute alpha-weighted RGB and mean alpha with deterministic half-up rounding. Canonicalise a fully transparent result to zero RGBA so hidden RGB cannot leak through later processing.
5. Composite pixelations below ordinary annotations. Later pixelation regions win in overlaps but always sample the same sanitised source, not another pixelation or annotation's output.
6. Apply opaque redactions last, then the existing crop and presentation pipeline. Redaction remains a separate tool and privacy boundary.
7. Bound aggregate eligible sampling across all pixelation regions to 256 million source-pixel visits per render. Reject excessive work explicitly; retain existing image/annotation limits.

Required invariants include identical visible output when only redacted or cropped-out source pixels change; stable region geometry at zoom; output route parity; retained original files; and unchanged ordinary-annotation ordering. Pixelation reduces detail and is not a guarantee that sensitive content cannot be inferred. Originals and undo state are retained.

## Verification and acceptance mapping

| Story | Implementation evidence | Native or observational evidence still required |
| --- | --- | --- |
| #23 Cancel | Gesture rollback, property transaction, capture cancellation/process cleanup tests | System selector pointer/Escape, capture exclusion, focus and nested-control behaviour |
| #24 Close | Fresh versus file-backed provenance, Copy-not-save, failed/cancelled save and revision guards | Actual picker, repeated capture, session return and retained files |
| #25 Pixelate | Range/edge/alpha/grid/overlap tests, hidden-pixel invariance, geometry and shared output | Visible controls, native keyboard/VoiceOver, real output destinations |
| #26 Highlight | Selected/default isolation, fixed-alpha compatibility, transactional undo | Named swatch comprehension, custom-picker preview/cancel/focus |
| #27 Shell | Original functionality retained and minimum-layout checks | Actual native screenshots, scaling/appearance, contrast, participant comparison |
| #28 Icons | Original vectors, names and guarded existing command dispatch | Actual-size rendering, state/focus and VoiceOver; confusing-symbol task observations |

The full 64-ID mapping remains in the prototype spec. The [testing procedure](../TESTING.md#native-editor-story-acceptance) defines reproducible native tasks. Keep issues open for acceptance gaps rather than treating one successful unit suite as complete product validation.

## Retires

Replace the crowded word-only tool grid with the labelled rail and contextual inspector. Remove ineffective generic highlight Weight/alpha presentation, the permanently-false one-off capture cancellation callback and the need to leave editing by quitting the app. Retain useful text labels, native command guards, existing original files, collection controls and shared pixel rendering. Do not leave old and new editor control paths active in parallel.
