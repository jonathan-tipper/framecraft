# Everyday capture and editing

Beta 6 candidate, 22 November 2026. Extends the setup and text work in #32 and #14 with the direct-output contract in [#36](https://github.com/jonathan-tipper/framecraft/issues/36) and shortcut recording in [#37](https://github.com/jonathan-tipper/framecraft/issues/37). [Release evidence](../TEST-NOTES.md) distinguishes implemented behaviour from native acceptance.

## User and outcome
A Mac user should set up capture once, then use a shortcut from another app, select an area, and edit or save predictably. Captured and imported images share the editor. Source files remain intact.

## Scope and delivery
1. Repair text placement/editing and cancellation with atomic undo.
2. Establish shared button geometry and consistent icons for tools and capture routes.
3. Add independent everyday capture preferences, global shortcut and collision-safe saves.
4. Add native first-run setup, retain image on failed save, document and test actual behaviour.

Keep Rust/egui and the existing capture adapter. No cloud, account, login item, new permission bypass, scrolling capture or persistent annotation project format. The installed beta is not replaced merely by editing source.

## Design and interaction decisions
Reuse the existing canvas/rail/inspector structure. Tool rail controls fill one common width; command groups share a 36-point height, 8-point corners, 18-point icons and consistent padding. The primary capture action has stronger emphasis. Pair symbols with labels, retain accessible names and focus/selected/disabled states. Use cool neutral surfaces and cobalt accent; decorative animation and new font dependencies are unnecessary. Existing original vectors are extended for window, display, folder, settings and keyboard.

Text uses a familiar transaction dialog, so a separate browser prototype is bypassed: native focus, canvas coordinates and undo are the material risks. Text → click image → focused draft → Add text. New labels default to white text on a dark backing for legibility. Blank text cannot commit. Cancel/Escape leaves the document unchanged. Select a label → Edit text, or double-click → draft → Apply changes. One commit is one undo unit. Step controls are separate. The native candidate is the runnable review surface; do not count a unit test as user validation.

Setup is a 4-step native flow with a persistent step indicator:

| Step | Content | Validation and exit |
| --- | --- | --- |
| Screen access | Purpose, current permission, Request access, system settings, Check again | Permission is requested only explicitly. Setup can continue without granting; capture remains gated. |
| Shortcut | Record shortcut, Choose keys individually, or Use menu bar only | Capture one focused chord. Escape abandons it. A conflicting or unsafe shortcut can be replaced before Finish. |
| Save location | Folder, prefix, optional date grouping, example path | Probe a write destination for either saving mode. |
| Finish | Open editor, save then edit, Copy and return, or Save and return | Persist the choices and register the confirmed binding. Back retains the draft. Cancelling discards it. |

First normal launch shows setup unless completed. `--demo` skips automatic setup. Setup can always be reopened from the header and native Settings. The session shortcut editor remains separately labelled. Captures, imports and editor mutations are blocked underneath setup; closing it retains the prior document. Skipping does not create preferences or register a global key.

## Data and lifecycle contract
Store the everyday binding and output preferences separately from session shortcuts. Preserve existing Edit and Save variants when reading beta 5 settings. If an old binding would consume ordinary typing or a standard edit command, disable it for review while keeping the other preferences. Display native registration failure separately from “off”. Register the replacement while retaining the previous key. Commit the preference write before releasing the previous registration. Escape, a busy chord or a failed write preserves the working binding.

A direct grab never replaces the current editor document. Capture completion publishes one host-only pasteboard image or a collision-safe PNG, then updates the single in-memory last capture. A failure preserves the captured image for retry or editing. A cancelled selection before publication leaves prior output and the editor untouched. Once publication finishes, the output belongs to the user. The two editor-opening modes retain their original document and save guards. Session, modal and in-flight capture guards apply to every entry point.

The native thumbnail is optional, accepts an explicit Edit action and stays out of the next selection. The menu always provides a keyboard-accessible alternative when a retained image exists. Clear or quit releases the retained in-memory item. An explicit editor copy uses the ordinary pasteboard. Direct copy requests the host-only pasteboard option.

Finishing setup validates and atomically replaces `quick-capture.json`. Malformed preferences require an explicit replacement. Session start and stop leave the one-off binding alone, and the two sets of keys must not collide.

Original PNGs use the configured safe prefix, UTC `YYYYMMDD-HHMMSS`, a collision suffix when needed, and optional `YYYY-MM-DD` folders. Saving publishes without overwriting an older file. Importing or reopening an image never triggers a one-off automatic save. In save-then-edit mode, failed saving opens the unsaved image for a manual export.

## Editing afterwards
Open Image, drag-and-drop and session reopening support later annotation of PNG/JPEG pixels. Existing text objects are editable while their document remains open. PNG exports flatten annotations; reopening one cannot recover its text objects or undo history. A durable editable document format and recent single captures are follow-on backlog work, not implied by this slice.

## Verification and open evidence
Automated: text add/edit/cancel, blank rejection, meaningful undo/redo, modal isolation, preference round-trip/invalid data, key conflicts, output collisions/path traversal, direct-save failure retention and no import/session auto-save. Existing redaction/render/capture/session regressions remain required.

Native: first launch, all 4 steps, keyboard focus, 900x620 layout, cancel/reopen settings, real background shortcut, selector completion/Escape, exact saved path/pixels, text edit/export and relaunch. Record which were observed in TEST-NOTES. VoiceOver, multi-display, supported macOS versions and usability comparison remain separate acceptance checks.

## Retires
Remove text-before-click requirements, mixed Text/Step properties, label-width-dependent rail buttons, session-only shortcut discovery and a fixed `Framecraft.png` suggestion for one-off exports. Preserve session settings, command guards, original retention and flattened export semantics.
