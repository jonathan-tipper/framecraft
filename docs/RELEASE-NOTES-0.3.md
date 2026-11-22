# 0.3.0-beta.6 capture and shortcut candidate

- **Copy and return** places the selected pixels on this Mac’s clipboard for pasting in the source app. **Save and return** writes a named PNG and retains the clipboard. Both offer **Edit Last Capture…** from the menu bar. Existing editor-opening preferences carry over.
- **Record shortcut** accepts an actual key combination. A labelled picker and **Use menu bar only** provide alternatives. Settings and the status menu show the binding that registered. A conflict or failed save retains the previously working key.
- **Screen Access** displays the running version and executable path. Development DMGs have a distinct filename. The Developer ID release path validates the signed, notarised app and DMG before producing distribution artefacts.

On multiple displays, interactive area and window grabs skip the thumbnail when display identity is ambiguous. The menu still provides **Edit Last Capture…**. macOS can report an exclusive shortcut conflict. Other utilities using shared registrations can accept the same keys, so the setup includes a rehearsal in the intended source app.

See the [MacBook acceptance check](TESTING.md#beta-6-shortcut-and-paste-on-another-mac), [evidence](TEST-NOTES.md) and [distribution status](https://github.com/jonathan-tipper/framecraft/issues/35).

# 0.3.0-beta.5: everyday capture and text correction (unreleased)

- Text opens a focused draft after placement, with explicit Add/Apply/Cancel, double-click editing and atomic undo. Step controls are separate.
- First-run Capture setup covers screen access, an independent global area shortcut, destination, naming, UTC date folders and optional automatic saving.
- Automatic saves never overwrite earlier captures. A failed save keeps the captured image available for manual recovery. Imported and session images retain their existing behaviour.
- Shared full-width tool buttons, consistent capture action sizing, additional labelled icons and cool neutral surfaces.
- [Implementation contract](specs/everyday-capture.md), updated guide and test evidence. Persistent editable documents remain backlog [#33](https://github.com/jonathan-tipper/framecraft/issues/33).

This candidate is not a published release. Native acceptance and distribution limits are recorded in TEST-NOTES; Developer ID signing/notarisation remain outstanding.

# 0.3.0-beta.4: screen-access recovery (unreleased)

Fixes the missing explicit screen-recording request flow. More Capture → Screen Access offers Request Access, Open System Settings and Check Again. One-off capture checks permission before hiding Framecraft; the shared capture adapter checks again before starting the native command, including session captures. Denied/unrequested access preserves the existing document and opens recovery. Command errors retain their diagnostic without asserting that every failure is a permission problem.

No permission is approved automatically. The full first-grant/denial/restart cycle must be checked on the affected Mac. Ad-hoc signing and cross-build permission identity remain distribution limits. See [installation recovery](INSTALLATION.md#allow-capture) and [test evidence](TEST-NOTES.md).

# 0.3.0-beta.3: editor experience (unreleased)

Development implementation of [epic #22](https://github.com/jonathan-tipper/framecraft/issues/22), using the [reviewed design candidate](specs/editor-experience-prototype.md). No beta 3 release or notarised download is published by this change. Build the current branch using the README instructions.

- Canvas-centred editor with warm neutral panels, a labelled vector-icon tool rail and contextual properties.
- Visible Cancel tool and Close screenshot, with unsaved-output protection and separate session-return behaviour.
- Rectangular pixelation with source-pixel block size, selected-object editing and undo. Opaque Redact remains separate and renders last.
- Named highlight colour presets and custom RGB, with selected-mark properties kept separate from next-highlight defaults.
- Capture cancellation, document lifecycle and compositor regression coverage, with actual results and remaining native checks recorded in [TEST-NOTES](TEST-NOTES.md).

The Rust/egui stack, macOS 13 deployment baseline and original-image retention remain. Browser prototype evidence does not establish native accessibility or capture behaviour. VoiceOver, participant comprehension, receiving-app and supported-Mac checks need their own evidence; the implementation stories remain open where that evidence is missing. Pixelation reduces detail and is not guaranteed to conceal sensitive content.

## 0.3.0-beta.2: Mac capture and installation

This beta responds to first-use feedback in [#20](https://github.com/jonathan-tipper/framecraft/issues/20). Download the Apple silicon DMG from [the prerelease](https://github.com/jonathan-tipper/framecraft/releases/tag/v0.3.0-beta.2). Drag the app into Applications; no Rust tools are needed.

- Screenshot capture is the initial screen. Area, Window and Screen commands are visible and available from a capture-frame menu bar icon.
- Native application menus expose capture actions and their Mac shortcuts. One-off capture is unavailable during active collection or conflicting operations.
- Shortcut settings use Command/Option names, modifier checkboxes and a key selector. Existing settings retain their meaning.
- The UI uses the installed Mac system font, light neutral surfaces and a clearer hierarchy. New captures preserve original dimensions; background styling is optional.
- Sessions use numbered setup, visible source/status and optional collection tuning. Copy/save stay visible in the editor; secondary actions are grouped.
- App icon, installation/update script and DMG packaging are included. Menus and normal quit retain edit/persistence protection.

See [beta 2 checks](TEST-NOTES.md) and the [Mac experience specification](specs/0.3-mac-experience.md). The app is still ad-hoc signed, not notarised. Broader permission/display/receiving-app/VoiceOver, Intel and older macOS checks remain open. Fixed-region and change-mode limits below still apply.

## Beta 1 feature foundation

This is an Apple silicon prerelease for testing. It is ad-hoc signed and is not notarised. Download the bundle and checksum from the [GitHub prerelease](https://github.com/jonathan-tipper/framecraft/releases/tag/v0.3.0-beta.1), or use the [README build steps](../README.md#try-the-build). Exact check results and remaining failures belong in [TEST-NOTES.md](TEST-NOTES.md).

## Added and changed

- Reopenable session folders with stable capture identity, schema-versioned metadata, recent-folder references, original/edited variants and confirmed import/recovery.
- Menu bar status and session controls, configurable global shortcuts, saved capture defaults with source reconfirmation, and source/destination recovery guidance.
- Source changes blocked during active sessions and Editor commands scoped to the visible Editor.
- Selection and movement for existing annotations, plus multiline text labels and numbered steps through the shared raster compositor.
- Full-text local OCR from current rendered pixels, with a review step and explicit copy. A bounded contrast pass addresses the reproduced dark-interface OCR failure without changing export pixels. Filename heading selection remains a separate operation.
- Reviewed image handoff through copy, neutral-name temporary file drag and Finder reveal.
- Selected offline HTML/PNG evidence packs with explicit order/captions, neutral names, bounded size and staged publication outside the source session folder.
- README, user/developer guides, architecture, testing procedure and bundled-font notices.

The implementation is scoped by [the 0.3 specification](specs/0.3-session-workbench.md) and [GitHub backlog](https://github.com/jonathan-tipper/framecraft/issues/1). Feature implementation does not close acceptance criteria that require native or participant evidence.

## Compatibility and data

macOS 13 remains the deployment baseline. The native synthetic OCR result was recorded on macOS 26.6.2, arm64; the supported-version, multi-display, permission, shortcut, receiving-app and accessibility matrix is not complete.

Session metadata schema 1 is new. Legacy folders preview recognised timestamp/sequence PNGs before confirmation; arbitrary filenames and unattached edits are not imported automatically. Unknown schema versions fail without migration. Keep the whole source folder and manifest together for backup. Metadata recovery cannot restore deleted pixels or annotation layers.

Originals remain on disk. Saved edited copies, copied images and packs contain flattened pixels. Redaction does not erase the retained original or in-memory undo source. Save an edited variant before including it in a session pack; unsaved document layers are not persisted across restart.

## Known limits and outstanding gates

- Capture targets a fixed display rectangle. Window tracking and stable display identity remain [#8](https://github.com/jonathan-tipper/framecraft/issues/8); equal-sized display reordering is not detected.
- Change mode saved 22 of 30 intended frames in each of 3 synthetic runs, with 8 misses and no duplicate or transition saves. Small/low-contrast changes were missed. See the [measured corpus and limits](evidence/change-mode.md); there is no completeness claim.
- Text uses embedded Hack with `?` fallback, explicit lines and no complex-script shaping/wrapping. OCR order is approximate and installed Vision determines language support.
- Images are RGBA8 without colour-managed wide-gamut/HDR handling.
- Up to 32 prepared drag files remain in temporary storage until exit; cleanup is best effort. Receiving applications and clipboard managers control their own copies.
- Pack export supports up to 100 frames and 64 million combined pixels. Another-Mac offline inspection remains a release check.
- The human [pilot #9](https://github.com/jonathan-tipper/framecraft/issues/9) is outstanding. No demand, usability or willingness-to-pay claim follows from competitor features.
- [Scrolling #18](https://github.com/jonathan-tipper/framecraft/issues/18) remains discovery. Video/audio, hosted sharing, accounts, automatic summaries, retained OCR indexing and editable projects are outside 0.3.
- The bundle is ad-hoc signed for local tests. Developer ID signing, notarisation and clean-Mac distribution are not supplied by the local build script.

Follow the [user guide](USER-GUIDE.md) for recovery and report reproducible failures with synthetic fixtures, build/commit and OS/hardware. Do not attach private captures or OCR transcripts to public GitHub issues.
