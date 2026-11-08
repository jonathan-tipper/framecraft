# Everyday capture and editing

Status: implementation candidate, 8 November 2026. Owns [#32](https://github.com/jonathan-tipper/framecraft/issues/32); extends #14 text correction and #27/#28 visual consistency. User feedback remains acceptance evidence of a problem, not proof that this candidate solves it.

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
| Shortcut | Enable everyday shortcut and readable modifier/key controls | Valid key, app-reserved and session conflicts checked before finishing; native registration failure retains draft. |
| Save location | Folder chooser, prefix, optional date grouping, example path | Absolute destination, bounded safe prefix. Finish probes destination only when automatic saving is chosen. |
| Finish | Open editor (default) or Save automatically; summary | Finish persists and registers; Back retains draft; Not now/Cancel discards draft. |

First normal launch shows setup unless completed. `--demo` skips automatic setup. Setup can always be reopened from the header and native Settings. The session shortcut editor remains separately labelled. Captures, imports and editor mutations are blocked underneath setup; closing it retains the prior document. Skipping does not create preferences or register a global key.

## Data and lifecycle contract
Store `quick-capture.json` separately from existing session shortcuts. Fields: setup completion, shortcut enabled/key, destination, filename prefix, date grouping and after-capture choice. Validate and atomically replace the file. Surface malformed settings rather than silently replacing them; an explicit Finish is required to adopt a replacement.

Register the everyday area shortcut only after completed setup. Keep its registration independent of session hotkeys and restore the prior binding on failed replacement. Guard every native command through the existing app capture state. Session start/stop must not unregister everyday capture. Reject saved session keys that conflict with the everyday key.

Filename contract: safe prefix + UTC `YYYYMMDD-HHMMSS` timestamp + collision suffix + `.png`; optional UTC `YYYY-MM-DD` directory. Explain UTC beside the preview. Atomic no-clobber publication never replaces an existing screenshot. Save original capture pixels with no style or annotation applied. Cancellation before atomic file publication produces no output; after publication, the completed save is retained and reported. Imported and session images never enter this auto-save route.

With Open editor, the capture is unsaved until the user exports. With Save automatically, save on the worker and then open the captured image for optional annotation. Success reports the real path and marks the source revision saved. Failure still opens the captured image as unsaved and gives a manual Save PNG recovery. Later edits use explicit Save PNG and do not overwrite the auto-saved original. A fresh capture still obeys unsaved-image and active-session guards.

## Editing afterwards
Open Image, drag-and-drop and session reopening support later annotation of PNG/JPEG pixels. Existing text objects are editable while their document remains open. PNG exports flatten annotations; reopening one cannot recover its text objects or undo history. A durable editable document format and recent single captures are follow-on backlog work, not implied by this slice.

## Verification and open evidence
Automated: text add/edit/cancel, blank rejection, meaningful undo/redo, modal isolation, preference round-trip/invalid data, key conflicts, output collisions/path traversal, direct-save failure retention and no import/session auto-save. Existing redaction/render/capture/session regressions remain required.

Native: first launch, all 4 steps, keyboard focus, 900x620 layout, cancel/reopen settings, real background shortcut, selector completion/Escape, exact saved path/pixels, text edit/export and relaunch. Record which were observed in TEST-NOTES. VoiceOver, multi-display, supported macOS versions and usability comparison remain separate acceptance checks.

## Retires
Remove text-before-click requirements, mixed Text/Step properties, label-width-dependent rail buttons, session-only shortcut discovery and a fixed `Framecraft.png` suggestion for one-off exports. Preserve session settings, command guards, original retention and flattened export semantics.
