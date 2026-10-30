# Product direction

Framecraft tests a specific proposition: people collecting meeting, presentation or software-evidence frames can spend less effort capturing, finding and explaining them if collection and review share one local workflow. Version 0.3 adds enough of that workflow to test the proposition. It does not establish that Framecraft is faster or easier than existing tools.

## Evidence and decisions

The [GitHub product brief](https://github.com/jonathan-tipper/framecraft/issues/1) is the backlog index and research record. Official product documentation was reviewed on 28 October 2026 for Apple Screenshot, CleanShot X, Shottr, Snagit, Loom, Screen Studio and Cap. These are benchmarks in different segments, not a defensible Mac market-share ranking. Vendor adoption claims are labelled as such in the brief. No competitor applications were benchmarked hands-on in that research.

The useful patterns are rapid handoff after capture; recoverable local history; revisable annotations; explicit OCR copying; and selected, ordered explanatory output. Apple already provides familiar basic capture, so adding another capture button alone offers little reason to switch. Video and hosted sharing introduce a different cost and privacy commitment and are outside this version.

## First-use correction

On 29 October, the user could not interpret shortcut names and found the session-first interface clunky. [Issue #20](https://github.com/jonathan-tipper/framecraft/issues/20) changes the initial task to one-off capture, adds Mac application/menu bar controls and installation, and replaces raw key syntax. This is direct feedback from one user; it does not complete the planned pilot. See the [Mac experience specification](specs/0.3-mac-experience.md).

## Delivery shape

| Outcome | 0.3 test-build scope | Evidence still needed |
| --- | --- | --- |
| Capture intended content | Confirmed fixed display area; manual, timer and change modes; menu bar controls; clearer recovery | Native permission, minimisation, display and shortcut matrix, [#7](https://github.com/jonathan-tipper/framecraft/issues/7) |
| Return and explain | Reopenable session metadata, original/edited variants, selection, text and steps | Reopen/recovery tests plus human correction and retrieval tasks |
| Share deliberate output | Reviewed image copy/drag, local OCR preview, selected offline HTML/PNG pack | Native receiving-app tests and offline inspection on another Mac |
| Establish user value | A working beta for the planned comparison tasks | Consented [pilot #9](https://github.com/jonathan-tipper/framecraft/issues/9), not synthetic claims |

The [0.3 specification](specs/0.3-session-workbench.md) records implementation choices and boundaries. GitHub issues remain the work queue. Code completion alone does not satisfy native or human-study acceptance criteria.

## Boundaries

A selected area is a rectangle in screenshot pixels, not a tracked window. Equal-sized display reordering is not detected. [Targeting research #8](https://github.com/jonathan-tipper/framecraft/issues/8) must resolve stable identity, window-following and compatibility before replacing the adapter. macOS 13 remains the baseline; setting a deployment target is not evidence of runtime compatibility on that release.

Change detection counts changed thumbnail pixels, not slide meaning. [Evaluation #12](https://github.com/jonathan-tipper/framecraft/issues/12) measures misses, duplicates and transitional saves independently. A synthetic corpus can expose regressions, but it cannot prove meeting completeness.

Apple Vision supplies local OCR. Heading selection proposes filenames; full-text recognition supplies a user-reviewed transcript from rendered pixels. There is no generative naming, automatic summary, searchable OCR archive or transcript sidecar.

Sessions retain original and edited PNGs plus bounded metadata. Editable-project persistence, batch styling, pinning, pixel rulers, video/audio, webcam composition, hosted links, accounts and billing are not part of 0.3. [Scrolling capture #18](https://github.com/jonathan-tipper/framecraft/issues/18) remains demand-led discovery after the pilot, with no shipping scrolling feature promised.

## Measurement and release claims

The following remain targets rather than measured results: under 1 second from manual shortcut to durable PNG after initial minimisation; under 1 second for a typical 4K export; and complete, correctly ordered deliberate captures. Record hardware, macOS, display scale, fixture size, mode and failures with timings. Do not revise targets silently to fit results.

The pilot compares task completion, missed/duplicate captures, correction work, retrieval and handoff effort against each participant's current tool. Its pre-registered provisional criteria include at least 80% unaided completion, no original-content disclosure, and a median improvement of at least 20% in the chosen primary measure without worse completeness. These are study criteria, not product claims. A small directional study does not establish population-wide usability.

A wrong-source capture or accidental original disclosure blocks a release claim regardless of speed. Keep those failures visible in [test notes](TEST-NOTES.md), issue acceptance evidence and [release notes](RELEASE-NOTES-0.3.md). The current distribution aim is an inspectable, ad-hoc signed test build and a reviewable pull request. Developer ID signing, notarisation and a separate-Mac installation check are later distribution gates.
