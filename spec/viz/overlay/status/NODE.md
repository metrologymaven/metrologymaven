---
id: VIZ.OVERLAY.STATUS
title: Status markers, locked palette guard and provenance badges
level: 3
parent: VIZ.OVERLAY
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-overlay]
built_pr: 355
satisfies: [REQ-VIZ.OVERLAY-005, REQ-VIZ.OVERLAY-006, REQ-VIZ.OVERLAY-008]
covers: []
depends_on: [VIZ.RENDER.SCENE.PROJ, VIZ.RENDER.SCENE.STALE, UNC.DEC]
uses_artifacts: [UX.TOKENS.STATUS, UX.TOKENS.DATAPAL, GDT.CHAR.INTERP]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-20
---

# VIZ.OVERLAY.STATUS: Status markers, locked palette guard and provenance badges

## Purpose

One pure mapping from a kernel-supplied status and provenance to what the user sees: an icon, a pattern, a text equivalent and a colour from the locked status palette. A guard rejects any palette, theme or skin that would draw a nonconforming or out-of-tolerance state in a conforming colour. Every overlay, the shell and the kiosk use this mapping, so the rule lives in one place.

## Scope

**In:** the `StatusGlyph` mapping for the six display states (CONFORMS, NONCONFORMS, INDETERMINATE, NOT_DECIDABLE, not evaluated, stale); the `ProvenanceBadge` mapping for five provenances (computed, measured, entered, reported by source, suggested); the palette guard; the definitions of the out-of-tolerance patterns (`pattern.out-of-tolerance` and its refinements `pattern.under-tolerance` and `pattern.over-tolerance`); the provisional-interpretation label of GD&T results (D-045) and the claimed-value styling.
**Out:** the theme token system and theme switching (`VIZ.SHELL`); computing statuses (`GDT`, `SCAN`, `UNC`); deciding whether a record is provisional (`fid_gdt::requires_provisional_label_bytes`, GDT.CHAR); colour maps and drawing the out-of-tolerance patterns on regions (`VIZ.OVERLAY.CMAP`); glyph and pattern artwork (`UX.ICONS.CLASS`).

## Requirements

- **REQ-VIZ.OVERLAY.STATUS-001** Every status display state shall map to a distinct icon, a distinct pattern and a text equivalent in addition to its colour.
- **REQ-VIZ.OVERLAY.STATUS-002** If a palette, theme or skin assigns a nonconforming, out-of-tolerance or stale state a colour within the configured perceptual distance of any locked conforming-set colour, or sets a conforming-set colour at all, then the guard shall reject it with a typed error and keep the locked default.
- **REQ-VIZ.OVERLAY.STATUS-003** Stale and not-evaluated states shall never be rendered with the conforming or nonconforming colour, icon or text.
- **REQ-VIZ.OVERLAY.STATUS-004** Every displayed number passed through the overlay API shall carry exactly one provenance badge from the five provenances, and an unbadged number shall be rejected at build time of the overlay item.
- **REQ-VIZ.OVERLAY.STATUS-005** The mapping shall be a pure function of status, provenance and theme with no dependence on UI toolkit state.
- **REQ-VIZ.OVERLAY.STATUS-006** When a GD&T result is shown and `fid_gdt::requires_provisional_label_bytes` returns true for its stored record bytes, the overlay shall render the provisional label (`gdt.interpretation.provisional`) and the provisional cue (D-045). A caller showing a GD&T result must build it as a `GdtResultItem` with the predicate computed from the record bytes; the status-marker type cannot carry an interpretation.
- **REQ-VIZ.OVERLAY.STATUS-007** The overlay shall never render with the measured styling a value that is claimed or rests on declared or uncharacterized input (the kernel's evidence facts): such a value shall get the claimed colour role, the word "claimed" and, for each input fact, its evidence label (`label.declared-input`, `label.uncharacterized-input`), and badging it `measured` shall be rejected at build time.
- **REQ-VIZ.OVERLAY.STATUS-008** The overlay shall define distinct patterns `pattern.under-tolerance` and `pattern.over-tolerance`, each refining the generic `pattern.out-of-tolerance` of UX.TOKENS.STATUS, for VIZ.OVERLAY.CMAP to draw on out-of-range regions.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/status-matrix` (6 states × 5 provenances × 4 themes: light, dark, high-contrast, shop-floor), when the mapping runs for every combination, then every state has a unique icon, unique pattern and non-empty text, identical across themes except colour.
- **AC-2** Given skin fixture `fixtures/viz/skin-azure-nonconform.json` that sets NONCONFORMS to the conforming azure, when the guard validates it, then it fails with `LockedPaletteViolation { state: NONCONFORMS }` and the active palette remains the locked default.
- **AC-3** Given the CIEDE2000 threshold of 20 [unverified] in the guard config, when a skin sets INDETERMINATE to a colour within that distance of the CONFORMS azure, then the guard rejects it.
- **AC-4** (error path) Given an overlay item with a number and no provenance, when it is built, then it fails with `MissingProvenance` and is not drawn.
- **AC-5** Given the same inputs, when the mapping runs twice, then outputs are identical (pure function, D0).
- **AC-6** Given stored GD&T record bytes for which `fid_gdt::requires_provisional_label_bytes` returns true, when the result is rendered, then the output carries the provisional label key, its text and the provisional cue; for a signed-off record it carries the signed-off line and no cue.
- **AC-7** (error path) Given a claimed value, when it is rendered with any provenance other than `measured`, then it gets the claimed role and the word "claimed"; when it is built with the `measured` badge, it fails with `ClaimedAsMeasured`.
- **AC-8** Given the pattern definitions, then `pattern.under-tolerance` and `pattern.over-tolerance` differ from each other and from every state pattern, and both refine `pattern.out-of-tolerance`.
- **AC-9** (deferred to UX.ICONS.CLASS) Given the icon index, every glyph and pattern ID this mapping uses exists in it.
- **AC-10** (error path) Given `fixtures/viz/skin-inversion.json` (CONFORMS red, NONCONFORMS azure), when it is read, then it fails with `SkinSchema` (a skin may not set CONFORMS), and a skin built in code that sets CONFORMS is refused the same way with the palette unchanged.
- **AC-11** Given the scene status `kernel:INDETERMINATE:-du` and a computed number, when it is rendered, then the value has the claimed role, the word "claimed" and both evidence labels.
- **AC-12** (deferred to VIZ.RENDER.SCENE.STALE) Given a scene node whose status is stale, when it is mapped, then it renders the stale glyph, pattern and text.

## Design

- **Registry command(s):** none (internal mapping used by overlays, shell and kiosk).
- **Data structures:** `DisplayState { Conforms, Nonconforms, Indeterminate, NotDecidable, NotEvaluated, Stale }`; `Provenance { Computed, Measured, Entered, ReportedBySource, Suggested }`; `Evidence { claimed, declared_input, uncharacterized_input }`; `StatusGlyph { glyph, pattern, text_key, text, color }`; `Badge { glyph, text_key, text }`; `PaletteGuardConfig { min_delta_e2000, min_neutral_delta_e2000, min_stale_delta_e2000_normal }`; `PatternDef { id, kind, refines }`; `Interpretation { Provisional, SignedOff }`; `OverlayItem` (status marker) and `GdtResultItem` (GD&T result, interpretation required), each with a builder and private fields; `RenderSpec` (`status-render@1`, canonical JSON and digest, private fields).
- **Algorithm:** table lookup for icon, pattern and text; colour from the locked palette, overridable by a theme only for states and colours that pass the guard; the guard computes CIEDE2000 (Sharma, Wu and Dalal, 2005) between each non-conforming-set colour and every conforming-set colour.
- **Errors:** `LockedPaletteViolation { state }`, `MissingProvenance`, `UnknownTheme`; as built also `ProvenanceWithoutNumber`, `ClaimedAsMeasured`, `NonFiniteNumber`, `KernelError`, `SkinSchema` (also a skin that sets CONFORMS).
- **Determinism:** D0 for the mapping (pure table lookup). The guard's f64 colour differences (no FMA) use the standard library's `powf`, `cbrt`, `atan2`, `sin`, `cos` and `exp`, whose last bits may differ between platforms, so a skin colour within about 1e-12 of a threshold could get a different verdict on another platform; no shipped or fixture colour is near that. Routing them through fid-num would remove this if the guard ever needs bit-identical verdicts.
- **Performance budget:** mapping is constant time; guard validation under 1 ms per skin.
- **Crate:** `fid-overlay` (module `status`).

## Test fixtures

`fixtures/viz/status-matrix/` (`make_fixtures.py`, `expected.json`) and the skins `fixtures/viz/skin-*.json` (among them `skin-azure-nonconform.json`): synthetic, CC-BY-4.0, written by an independent stdlib-Python oracle from the leaf's hand-labelled tables and the colours of the shipped `design/tokens/` files. The patterns are original definitions (parameters, no artwork), so ATTRIBUTION.md needs no entry.

## Build prompt

> In crate `fid-overlay`, add module `status`. Implement `status_glyph(status, theme) -> StatusGlyph` and `provenance_badge(provenance) -> Badge` as pure functions over six display states (Conforms, Nonconforms, Indeterminate, NotDecidable, NotEvaluated, Stale) and five provenances (Computed, Measured, Entered, ReportedBySource, Suggested). Each state has a unique icon, unique pattern and a localisable text key independent of theme; only colour varies by theme. Implement a palette guard: a theme or skin may not give a nonconforming, indeterminate, not-decidable, not-evaluated or stale state a colour within a configurable CIEDE2000 distance (default 20) of any conforming colour (the CONFORMS azure of UX.TOKENS.STATUS); reject with `LockedPaletteViolation` and keep the locked default. Overlay items that show a number must carry a provenance or fail with `MissingProvenance`. A GD&T result item takes the caller's `fid_gdt::requires_provisional_label_bytes` result and renders the provisional label and cue when it is true; a claimed value never gets the measured styling. Define `pattern.under-tolerance` and `pattern.over-tolerance` refining `pattern.out-of-tolerance`. Tests: full 6×5×4 matrix uniqueness; guard rejects the azure-nonconform skin and a near-azure indeterminate; missing provenance error; purity; provisional label; claimed styling. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable; clean-room icons only; status never by colour alone.

## Open questions

- The CIEDE2000 threshold value needs a review by an accessibility specialist; VIZ.SHELL owns theme tokens and must call this guard when loading skins.
- For VIZ.OVERLAY.CMAP (not editable from here): this leaf owns the out-of-tolerance pattern definitions (`pattern.out-of-tolerance`, `pattern.under-tolerance`, `pattern.over-tolerance`; `out_of_tolerance_cue`), and CMAP draws them on out-of-range regions together with the under/over colours of UX.TOKENS.DATAPAL. CMAP should declare `uses_artifacts: [VIZ.OVERLAY.STATUS]`.
- AC-9 is deferred to UX.ICONS.CLASS: the glyph IDs `state.<state>` and `badge.<provenance>`, the pattern IDs and `badge.provisional` are assumed until that node publishes its icon index; then a test must check they exist.
- The stale state reaches this mapping only once VIZ.RENDER.SCENE.STALE adds `Stale` to the scene's `DisplayStatus` (AC-12, deferred). Until then `from_display_status` maps the four kernel outcomes and `not_evaluated`. A kernel error is `KernelError`: the scene's error style draws it, never as a display state.
- VIZ.SHELL (not editable from here), the consumer, must test that every GD&T result it shows goes through `GdtResultItem` with `fid_gdt::requires_provisional_label_bytes` computed on the stored record bytes; this crate cannot see where a number comes from.
- As built (PR 355 review): the kernel's evidence facts (`claimed`, `declared_input`, `uncharacterized_input`) are carried through `from_display_status` into the item (`Evidence`). Any of them gives the claimed styling and forbids the `measured` badge, and each input fact adds its own label, so a value never looks measured where RPT.MODEL.BIND's `looks_measured()` is false.
- As built: the guard checks every slot other than CONFORMS (the five other states and the two out-of-range colours) against CONFORMS at 20 under normal vision and the three Machado 2009 dichromacies, as UX.TOKENS.STATUS and UX.TOKENS.CHECK verify the locked palette. It also checks indeterminate, not-decidable and not-evaluated at 10 against nonconforms and stale (UX.TOKENS.STATUS `STATUS-DISTANCE`, REQ-003). Stale must be at least 10 from nonconforms under normal vision (maintainer decision on PR 355; the locked palette gives 18.0 to 19.3) and not the same colour. Under simulated dichromacy stale and nonconforms only have to differ, because the locked palette itself has them 3.34 apart there (dark theme); the glyph, pattern and word tell them apart. Every distance to CONFORMS is measured against the locked CONFORMS, and a skin may not set CONFORMS (no palette inversion). A skin is applied all or nothing.
- As built: the locked colours are a table in the crate, kept equal to `design/tokens/status.json` and to the palette catalogue's `under`/`over` by a test (`uses_artifacts`). A token change that is not mirrored fails CI. No token was added, so the UX.TOKENS.CHECK gate is unchanged.
- As built: the claimed styling (`sem.text.value.claimed` and the word "claimed") applies when the kernel's `claimed` flag is set or the provenance is `entered`, `reported-by-source` or `suggested`. `computed` and `measured` get `sem.text.value.measured` unless claimed. This is a judgement call, to review with UNC.UEC.CLASS.
- As built: `fid-overlay` takes the provisional decision as an input (`Interpretation::from_requires_label`). The caller must supply `fid_gdt::requires_provisional_label_bytes(record)`. The layers would allow a runtime dependency (services is below viewer), but fid-gdt brings the journal and store into a pure mapping. The tests use fid-gdt as a dev-dependency, to feed the real predicate and to keep the label constants equal. A GD&T result item cannot be built without its interpretation.
- As built: skins are `{ "format": "status-skin@1", "themes": { "<theme>": { "<slot>": "#rrggbb" } } }`, the slots being the six states plus `under-tolerance` and `over-tolerance`.

## Change log

- 2026-11-20 (maintainer decisions for the build):
  - `depends_on` narrowed from `[VIZ.RENDER]` (a whole split node, broader than this pure mapping) to `[VIZ.RENDER.SCENE.PROJ]`, whose `DisplayStatus` the mapping reads.
  - `uses_artifacts: [UX.TOKENS.STATUS, UX.TOKENS.DATAPAL]` added for the locked colours.
  - AC-2, AC-3, the Build prompt and the skin fixture now say CONFORMS is azure (UX.TOKENS.STATUS, #339), not green; the fixture is `skin-azure-nonconform.json`.
  - Added REQ-006 and AC-6 (D-045 provisional label), REQ-007 and AC-7 (claimed styling), REQ-008 and AC-8 (under- and over-tolerance patterns: this leaf defines them, VIZ.OVERLAY.CMAP draws them), and AC-9 (glyph-ID existence, deferred to UX.ICONS.CLASS).

- 2026-11-20 (review of PR 355, maintainer decisions): skins may not set CONFORMS and distances use the locked CONFORMS (REQ-002, AC-10); the kernel's declared-input and uncharacterized-input facts are kept and labelled (REQ-007, AC-11); GD&T results are a separate type and REQ-006 states the caller's obligation; stale is at least 10 from nonconforms under normal vision (REQ-003); `depends_on` adds VIZ.RENDER.SCENE.STALE (AC-12, deferred) and UNC.DEC, `uses_artifacts` adds GDT.CHAR.INTERP; Determinism notes the platform caveat of the guard's colour maths.

## Children

_Leaf: none._
