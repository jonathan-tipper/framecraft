# Architecture: 0.3 test build

Framecraft is a Rust desktop application with an egui/eframe GPU-rendered interface. Rust adapters call macOS capture, Apple Vision OCR and native desktop controls. There is no webview, account service or application cloud endpoint. The [0.3 specification](specs/0.3-session-workbench.md) defines intended behaviour; [test notes](TEST-NOTES.md) record verification.

## Components

| Module | Responsibility |
| --- | --- |
| `app.rs` | Pages, document revision, selection/editing, unsaved-change decisions, background rendering/OCR, output review |
| `session_ui.rs` | Session setup, reopen/import preview, review rows, variant choice and session commands |
| `session_runner.rs` | Capture scheduling, generations, pause/resume/stop, limits and bounded heading-naming queue |
| `session.rs` | Atomic PNG storage, safe filenames, per-capture synchronisation and change detection |
| `library.rs` | Versioned manifests, stable capture IDs, file inspection, confirmation/recovery, folder locking and recents |
| `capture.rs` | Bounded system screenshot process, display geometry checks, cropping and capture-control visibility |
| `desktop.rs`, `settings.rs` | Main-thread menu bar/global shortcuts and session shortcut preferences |
| `quick_capture.rs`, `setup_ui.rs` | Independent one-off capture settings, UTC names, cancellable no-clobber PNG publication and native setup draft |
| `naming.rs` | Apple Vision observations, separate heading ranking and full-text reading order |
| `editor.rs` | Source-coordinate annotations, glyph rasterisation, validation, crop/style composition and flattened pixels |
| `output.rs` | Resolved pixel snapshots, neutral drag PNGs and staged offline packs |

## Capture and scheduling

Source selection captures a numbered display using `/usr/sbin/screencapture -D`, then records a physical-pixel rectangle and expected display dimensions. Each sample captures the selected display to a private temporary directory, decodes it, checks dimensions and crops before session persistence or naming. Cleanup is best effort. This adapter is not operating-system region-only capture and does not follow windows.

Display numbers are not stable identities. Dimension changes fail explicitly; equal-sized display swaps are not detected. A direct ScreenCaptureKit replacement requires a separate decision under [#8](https://github.com/jonathan-tipper/framecraft/issues/8).

The runner owns running, paused and stopped state. Generation counters invalidate stale requests/results across pause/resume/stop. An atomic persistence operation already underway may complete. Manual requests are bounded and tagged with the current generation. Interval scheduling includes processing time. Change mode samples roughly once per second and measures a 128 × 72 greyscale fingerprint, an intensity threshold, changed-area fraction, settling period and cooldown. The saved baseline advances only after persistence; resume resets it.

Main-thread native controls and in-window actions use the same runner commands. Successful start/resume minimises the window; focus return pauses capture. Unattended capture has a deadline; interactive selection remains user-cancellable. Source changes and shortcut replacement are blocked while a session is active. The menu bar item attempts to hide around capture, with an acknowledgement boundary rather than an unverified timing-only assumption.

## Durable sessions

Original PNGs are encoded into a same-directory temporary file, synced and persisted without clobbering existing captures. A successful PNG save is independent of later naming or metadata success. Metadata failure retains the acknowledged image and reports recovery guidance.

`framecraft-session.json` uses schema version 1. It stores session/capture identity, titles, sequence/timestamps, relative image filenames, lengths and fingerprints. Fingerprints detect accidental external changes; they are not an authenticity or tamper-resistance guarantee. Manifests contain no editable annotation layers or OCR transcripts. A per-folder file lock prevents concurrent writers; atomic metadata replacement and directory sync bound interruption behaviour.

Opening first inspects a bounded folder/manifest and generates a preview. Legacy import, orphan recovery, malformed metadata recovery and external relinking require confirmation. Unknown schema versions fail without migration. Invalid metadata is backed up before confirmed replacement. External replacement PNGs are copied into the session; traversal paths and symbolic-link image imports are rejected. Limits include a 4 MiB manifest, 10,000 captures, 20,000 inspected folder entries and 512 MiB per image file, followed by decoder pixel bounds.

Recent-session references live under Application Support and are capped at 12. Capture defaults and a source hint are saved separately from shortcut preferences; restoring them never restores an executable capture target, which requires confirmation again. Forgetting a reference does not remove source data. Session opening is review-only; it cannot restart capture. Review order and pack drafts are not an editable-project format.

`CaptureHandle` serialises each capture's manual rename, OCR rename and image loading. Manual names win over pending results. Background loading resolves the current file under the same per-capture lock. Distinct captures retain independent locks.

## Image and annotation model

`Document` owns an RGBA8 source image, optional source-coordinate crop, annotations and presentation style. Sources/outputs are bounded to 16,384 pixels per axis and 64 million pixels. Input PNG/JPEG orientation is applied; source metadata is discarded. ICC profiles are not colour-transformed, so colour-managed wide-gamut/HDR fidelity is outside the current contract.

The compositor draws arrows, rectangles, highlights, ink, text and steps; applies opaque redactions last; then crops and adds presentation styling. Preview, current-output export and OCR consume this shared rendered result. No export layer refers back to hidden source pixels. Redaction does not destroy in-memory source/undo data or retained original PNGs.

Text uses embedded Hack through `ab_glyph`. Layout is deterministic Unicode-scalar layout with explicit newlines and `?` fallback for missing glyphs, without wrapping or script shaping. Annotation bounds, arrow segments and rasterisation share geometry. Hit-testing follows visible order, with redactions first; index selection exposes obscured marks. Translation clamps a complete shape instead of distorting vertices. The UI groups user gestures into undo actions and tracks document revisions for stale-result rejection.

## OCR boundaries

Heading naming runs after original persistence on a separate bounded queue. It reduces input to 2,048 pixels per axis and ranks confident, meaningful heading candidates; failure leaves the fallback filename intact.

Full-text recognition is separate: it consumes current flattened pixels, reduces input to 4,096 pixels per axis, retains numbers/URLs/footer/interface text and orders approximate rows top-to-bottom, then left-to-right. The OCR working buffer composites alpha onto opaque white so hidden RGB channels cannot contribute. It samples at most 4,096 evenly spaced pixels and inverts RGB when at least 60% have integer luminance `(54R + 183G + 19B) / 256` below 96. This one-pass contrast heuristic addresses a reproduced dark-interface failure; it changes neither source/export pixels nor redaction/crop boundaries and does not guarantee accuracy on every layout. It rejects results beyond 512 observations or 32,768 Unicode characters. An empty result is distinct from failure. macOS Vision objects are created and released on the worker; no image file, network service or retained transcript is used.

The UI owns cancellation, current-revision checks and explicit Copy after preview. Cancellation invalidates a result; it does not promise immediate interruption of Apple's synchronous recognition call. Complex tables/columns need human review, and installed Vision controls language support.

## Output and publication

A `Snapshot` contains the resolved RGBA pixels for one output. **Original** and **Saved edited** variants decode the selected file unchanged; **Current edits** renders the current document once. Missing edited output fails without falling back to the original. Saved flattened files do not receive presentation styling again automatically.

Native file drag uses a neutral `frame-0001.png` in a private temporary directory retained until application exit. Clipboard, recipient storage and temporary cleanup have their own platform lifetimes. Finder reveal targets a saved file, not unsaved document state.

A pack freezes selected snapshots, order, explicit captions and variant labels before export. It emits only `index.html` and neutral sequential PNGs. HTML escapes user text and references local assets; it includes no scripts, source paths or automatic OCR-derived titles. The 100-frame/64-million-total-pixel limits bound work. Export canonicalises destinations, rejects the source folder and descendants, stages and syncs every output, then publishes using an exclusive native directory rename. Existing destinations are not overwritten. Cancellation/failure before publication leaves no completed pack.

## Release boundaries

The target is a local, ad-hoc signed macOS test bundle. Platform deployment flags are not native evidence. Permission behaviour, multiple display arrangements, menu hiding, minimised shortcuts, external drag recipients, VoiceOver and another-Mac offline packs remain explicit [test gates](TESTING.md). No performance or user-benefit claim follows merely from unit tests or a synthetic OCR pass.


## Beta 2 Mac entry points

The first window is single capture, with Sessions as a secondary view. Native application/menu bar actions route through the same guarded capture entry point. Capture Area uses the existing OS interactive selector; no new capture backend is introduced. App-visible capture shortcuts use the native menu accelerator on macOS. The three session bindings remain global only while a collection is active. Human-readable shortcut parts round-trip to the existing native key representation; saved legacy settings retain their meaning.

The icon and DMG/install scripts are distribution assets, independent of image storage. The installed bundle has the same `dev.framecraft.app` identifier and macOS 13 baseline. Ad-hoc signing is not notarisation.


## Everyday capture (beta 5 candidate)

`setup_ui.rs` renders a draft; `app.rs` validates, probes the destination when direct saving is selected, registers the independent area shortcut and atomically persists `quick-capture.json`. A failed persistence attempt restores the previous key or reports recovery failure. Session keys use their existing file and remain independent; conflicts are validated at both the preference and native boundaries.

The capture worker retains pixel ownership through optional automatic saving. `quick_capture.rs` writes a private temporary PNG, checks cancellation and atomically publishes without replacement. Cancellation before publication returns no image/file; publication is the commit boundary. Save failure returns the captured pixels to the editor as unsaved. Successful automatic saving marks only the just-loaded source revision saved; later annotation edits remain unsaved. The shared renderer continues to govern edited exports.

Text editing uses a draft transaction isolated from underlying commands. Add/Apply creates one history entry; Cancel never adds an empty annotation. The current PNG format remains flattened. See the [everyday capture contract](specs/everyday-capture.md).
