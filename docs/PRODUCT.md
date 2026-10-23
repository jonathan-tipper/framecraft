# Product direction

Version 0.2 centres the product on collecting screenshots during a meeting, demo, or presentation. The user chooses the shared-content area once and keeps working. Reviewing, naming, and annotation can happen afterwards.

Version 0.1 began with a single-image editor. That remains useful, but reopening selection, editing, and save dialogs for every slide creates unnecessary interruption. Sessions separate collection from later editing.

## What this version delivers

- A remembered display region, destination, and capture schedule for the active session.
- Manual global capture, timed capture, or settled visual-change capture.
- Immediate PNG persistence followed by optional local OCR naming.
- A thumbnail list with rename and edit actions.
- Highlights, ink, arrows, crop, redaction, and presentation styling.
- Edited copies beside the source images, preserving the original captures.

Rust and MIT licensing support inspectability and development. They do not establish that Framecraft is faster or easier to use than an existing tool. The session workflow needs to earn that claim in actual use.

## Deliberate limits

The selected area is fixed to screenshot coordinates, not attached to a meeting window. The app checks pixel dimensions, but it cannot detect an equal-sized display taking another display's index. Moving the shared content or rearranging displays requires stopping and selecting again.

Change detection measures changed thumbnail pixels, not slide meaning. Tiny text edits can be missed; animations can postpone settling. A manual capture remains available in every running mode.

Automatic names come from Apple Vision OCR and heading-selection rules. There is no local language model, semantic diagram understanding, or generated summary. Naming never delays saving the original image.

## Next milestones

| Milestone | Work | Evidence needed |
| --- | --- | --- |
| Validate 0.2 on a Mac | Capture permissions, minimisation, global keys, multiple displays, OCR, stop/resume | Recorded manual tests on named macOS versions and hardware |
| Improve source targeting | Direct ScreenCaptureKit adapter, stable display identities, optional window tracking | Repeatable Retina and display-reconfiguration tests |
| Improve collection | Review change thresholds against real presentations, clearer skipped-name status, recovery from failures | Capture completeness, false-save rate, and recovery tests |
| Improve review | Session reopening, editable projects, annotation selection, optional retained history | File round trips, retention behaviour, and usability tests |
| Prepare distribution | Signed and notarised releases, update policy, dependency review | Installation on a separate Mac and documented release checks |

Recording, scrolling capture, and remote sharing remain outside this release. A direct `SCScreenshotManager` adapter would target macOS 14 or newer; macOS 13 support would need a retained fallback or a separate implementation.

## Measurement targets

These are targets, not measured results. Record release-build hardware, display scale, image dimensions, and capture mode when testing.

| Measure | Initial target |
| --- | --- |
| Manual shortcut to durable PNG | Under 1 second after initial minimisation |
| Thirty-slide test deck | Every deliberate manual capture present and correctly ordered |
| Change mode on static slides | One saved image per settled slide, with measured misses and duplicates |
| Capture under slow or failing OCR | No lost PNGs; timestamp fallback retained |
| Editing a 3840 × 2160 frame | Responsive ink and under 1 second for a typical export |
| Native privacy behaviour | No application network requests for capture, editing, or naming |

Compare against the tools users already use. Measure missed moments, correction effort, naming usefulness, and whether a user accidentally shares an unredacted original alongside an edited copy.
