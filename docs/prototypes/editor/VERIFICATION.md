# Prototype review record

Date: 31 October 2026. This record concerns the browser design prototype, not the installed Rust application. The proposed UI is for review; native capture, filesystem publication and receiving applications remain separate engineering checks.

## Reference review

Rendered the unchanged `entry.html` for six studies from Form Atlas commit `cbdd4a1d189690c5aea5b0906a396b6c22a11b9b`, extracted into an isolated temporary directory. The working Form Atlas repository was not changed. Visual inspection used the in-app browser at its default 1280 × 720 viewport.

| Study | Observed design | Adopted principle |
| --- | --- | --- |
| d35 Monochrome project workbench | Narrow navigation and a large, aligned central workspace | Give the screenshot most of the available space; group secondary actions |
| d39 Live object inspector | Large object preview and narrow right-hand properties | Keep the current object and its relevant controls adjacent |
| a52 Collapsible labelled side rail | Symbols and visible navigation labels | Use icons with names and an explicit selected state |
| c24 Settings with live preview | Compact controls beside their visible result | Show property changes immediately and define their commit/cancel boundary |
| j07 Palette sampling loupe | Visible swatches and a prominent current colour | Make the current colour and selectable alternatives tangible |
| j18 Parametric icon forge | Consistent geometry at several display sizes | Inspect original icon strokes and alignment at actual control size |

The reference review establishes visual provenance. It does not establish usability or validate Framecraft's task flows. The prototype does not copy the studies' page layouts, editorial headings, code or assets.

## Browser verification

Review environment: Codex in-app browser on macOS, using pointer and keyboard input through its browser controls. The browser engine version was not captured. Viewports: 1280 × 720, 1200 × 800 and 900 × 620 CSS pixels. The default viewport was restored after testing. These were reviewer checks, not an unaided participant study.

| Check | Observed result |
| --- | --- |
| Highlight defaults versus selection | Set Next highlight to Green; add a mark; change the selected mark to Pink; leave and re-enter Highlight. Next remains Green. |
| Custom colour cancellation | Preview `#FF3300`, then Cancel. Opening Green returns and focus returns to Custom colour. |
| Leave a tool | Cancel tool returns to Select and keeps completed marks. |
| Copy then Close | Simulated Copy does not remove the unsaved decision. Cancel retains the screenshot; simulated Save and close returns to capture-ready. |
| Save failure and recovery | Direct failure fixture retains the highlight when choosing Keep editing. Injected next-save failure from a close decision preserves the document; Retry simulated save completes the return flow. No PNG is written. |
| Session return | Close the clean Capture 02 fixture, then open Capture 03 and close it. Titles match the selected card and focus returns to that card's Edit image button; no collection mutation is simulated. |
| Capture cancellation | Visible Cancel capture and Escape return to the prior empty state. Background controls are excluded from the active accessibility tree; Tab wraps within the selection overlay. |
| Capture selection | Pointer-drawn selection returns the synthetic editor fixture. The full fixture is returned, not a cropped output. Status text discloses this limitation. |
| Block-size validation and undo | Set 12 → 48 and commit with Enter; type 999 and try Enter; validation appears. Escape restores 48; Undo restores 12 without deleting the region. |
| Pixelation effect | Draw a second region over the orange artwork and commit 32 source pixels. Larger visible mosaic blocks appear; the inspector identifies the selected mark. |
| Minimum window | At 900 × 620, tool labels and Not saved remain visible, primary actions and close decisions are reachable, and there is no horizontal page or tool-rail overflow. Long inspector content scrolls. |
| Canvas fit | At minimum and larger review sizes, the whole image fits while preserving its aspect ratio. |
| Runtime diagnostics | No browser warning/error entries were returned after the final layout/contrast reload. |

### Corrections made during review

- Fit originally constrained only image width and clipped its height. It now uses both available dimensions; a 2 px allowance avoids fractional-layout overflow.
- The narrow tool rail clipped labels at 900 × 620. It now retains 132 px with compact padding and no horizontal overflow. The unsaved indicator also remains visible.
- Enter did not commit a numeric block-size transaction. Cancelling an invalid draft could consequently undo the preceding committed-looking adjustment. Enter now commits explicitly; the validation/Escape/Undo sequence above passed on retest.
- Some supporting text used insufficiently dark candidate colours. Those values now use the darker shared supporting-text token. Selected tools have an accent outline in addition to the changed surface and semantic state.
- The initial session image now names Capture 02, an item present in the synthetic return collection. Opening another card preserves its identity and Close returns focus to that card.

### Palette measurements

Calculated sRGB contrast ratios for opaque token pairs after the text-colour correction:

| Foreground / background | Ratio |
| --- | --- |
| Supporting `#5e646b` / canvas `#e9e7e1` | 4.84:1 |
| Supporting `#5e646b` / chrome `#f7f6f2` | 5.53:1 |
| Supporting `#5e646b` / panel `#fffefa` | 5.93:1 |
| Main `#24272b` / panel `#fffefa` | 14.86:1 |
| White / primary `#275adf` | 5.81:1 |
| Focus `#1549ca` / panel `#fffefa` | 7.33:1 |
| Destructive `#ad3434` / panel `#fffefa` | 6.27:1 |

These are palette-pair calculations, not a complete rendered-state accessibility audit. They do not measure disabled controls, all hover/pressed surfaces, screenshot content, native controls or assistive-technology behaviour.

### Screen evidence

All images contain synthetic content. The [gallery](README.md#screens-and-review-evidence) includes highlight, pixelation, close and minimum-size views.

| Scenario | Image |
| --- | --- |
| S01 empty | [Capture-ready](screenshots/empty.jpg) |
| S02 editor | [Tools, canvas and inspector](screenshots/editor.jpg) |
| S03 selection | [Visible Cancel capture](screenshots/capture.jpg) |
| S04 highlight | [Named swatches and selected target](screenshots/highlight.jpg) |
| S05 pixelation | [32 px region after drawing](screenshots/pixelate.jpg) |
| S06 close | [Save / Discard changes / Cancel](screenshots/close.jpg) |
| S07 failure | [Retained document and recovery](screenshots/save-error.jpg) |
| S08 session | [Origin image](screenshots/session.jpg) and [return collection](screenshots/session-return.jpg) |
| Minimum size | [900 × 620 editor](screenshots/minimum-window.jpg) |

Held-pointer Escape, every focus path, OS reduced-motion/high-contrast settings and native VoiceOver were not exercised. Reduced-motion/high-contrast rules were inspected in source only. The script in the design contract remains the participant-review protocol; its existence does not mean all its tasks have been observed.

### Source and documentation checks

- Extracted inline JavaScript and ran `node --check`: passed.
- Parsed HTML: 63 unique static IDs, no duplicate static IDs and no external loading assets.
- Checked the three new Markdown documents: all 40 local file links resolve.
- Checked the traceability table: 64/64 acceptance IDs, each mapped once. A separate product-manager review confirmed the eight scenario hashes and story anchors.
- Ran `git diff --check`: passed. This change does not modify Rust code, so no new Cargo test run is claimed.

## Native and research limits

- No Screen Recording permission, real native selector, system menu or global shortcut is exercised by the browser prototype.
- Save and Copy describe simulated results. They do not prove durable PNG publication, clipboard contents or receiving-app compatibility.
- Browser focus/semantics are not proof of native egui/AccessKit or macOS VoiceOver behaviour.
- Prototype pixel effects are illustrative. Rust compositor parity, hidden-pixel invariance, image limits and output metadata require the tests in the implementation stories.
- No participant study, performance claim or general macOS compatibility claim follows from this review.

Keep issues #22–#28 open until their actual product acceptance criteria have implementation evidence.
