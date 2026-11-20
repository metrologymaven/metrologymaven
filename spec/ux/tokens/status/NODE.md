---
id: UX.TOKENS.STATUS
title: Locked status palette values
level: 3
parent: UX.TOKENS
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-design]
built_pr: 339
satisfies: [REQ-UX.TOKENS-009]
covers: []
depends_on: [UX.SYSTEM.PKG]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-20
---

# UX.TOKENS.STATUS: Locked status palette values

## Purpose

The default colours for the six display states (CONFORMS, NONCONFORMS, INDETERMINATE, NOT_DECIDABLE, not evaluated, stale) in each theme, the split into a conforming set and a non-conforming set, and the perceptual-distance threshold handed to the runtime palette guard in VIZ.OVERLAY.STATUS. Colour is one cue of several; glyphs and text come from UX.ICONS.CLASS and the components.

## Scope

**In:** `tokens/status.json` (`status-palette@1`): per-theme colour per state, the conforming-set and non-conforming-set membership, the guard threshold and the colour-difference formula ID.
**Out:** the runtime guard and state-to-glyph mapping (`VIZ.OVERLAY.STATUS`); glyph shapes (`UX.ICONS.CLASS`); distance verification (`UX.TOKENS.CHECK`).

## Requirements

- **REQ-UX.TOKENS.STATUS-001** The file shall give one colour per display state for each of the four themes, and the semantic tokens `sem.status.<state>` shall alias these values.
- **REQ-UX.TOKENS.STATUS-002** The conforming set shall contain only CONFORMS, and the non-conforming set shall contain NONCONFORMS, out-of-tolerance region colours and stale; INDETERMINATE, NOT_DECIDABLE and not evaluated shall each use a hue that is in neither the conforming nor the nonconforming hue family.
- **REQ-UX.TOKENS.STATUS-003** The file shall state the guard threshold as `min_delta_e2000 = 20` [unverified threshold] and the formula ID `ciede2000`, and these shall be the values VIZ.OVERLAY.STATUS uses as its defaults.
- **REQ-UX.TOKENS.STATUS-004** Every non-conforming-set colour shall be at least the threshold from every conforming-set colour under normal vision and under simulated protanopia, deuteranopia and tritanopia, as reported by `UX.TOKENS.CHECK`.
- **REQ-UX.TOKENS.STATUS-005** The brand tier shall contain no status token, and the file shall be marked `locked = true`.

## Acceptance criteria

- **AC-1** Given the shipped `tokens/status.json`, when `fid-design status-check --root design --json` runs, then it exits 0 and lists 6 states × 4 themes.
- **AC-2** (error path) Given fixture `fixtures/ux/tokens/status/missing-hc/` (no high-contrast colour for stale), when the check runs, then it exits 1 with `STATUS-MISSING hc stale`.
- **AC-3** (error path) Given `unlocked/` (`locked = false`), when the check runs, then it exits 1 with `STATUS-UNLOCKED`.
- **AC-4** Given the shipped file, when `UX.TOKENS.CHECK` runs, then every non-conforming-set colour is at least ΔE2000 20 from CONFORMS in every theme under the four vision conditions.

## Design

- **Command:** `fid-design status-check --root <dir> [--json]` (structure, set membership, lock). Tooling, nearest class R0.
- **Format:** `{ format: "status-palette@1", locked: true, formula: "ciede2000", min_delta_e2000: 20, sets: { conforming: [..], nonconforming: [..] }, themes: { light: { conforms: "#..", ... }, ... } }`.
- **Errors:** `STATUS-MISSING`, `STATUS-SET`, `STATUS-UNLOCKED`, `STATUS-THRESHOLD`, `STATUS-SCHEMA`.
- **Determinism:** D0.
- **Crate:** `fid-design`, module `status`.

## Test fixtures

`fixtures/ux/tokens/status/`: `missing-hc/`, `unlocked/`; synthetic, CC-BY-4.0. Built with 19 further error-path fixtures and `expected.json`, written by `make_fixtures.py` (hand-labelled findings; CIEDE2000, colour-vision, contrast and hue values of the shipped palette from an independent Python reading).

## Build prompt

> Author `tokens/status.json` (`status-palette@1`, `locked: true`, formula `ciede2000`, `min_delta_e2000: 20`) with original colours for CONFORMS, NONCONFORMS, INDETERMINATE, NOT_DECIDABLE, not_evaluated and stale in themes light, dark, hc and floor; conforming set {conforms}; nonconforming set {nonconforms, out_of_tolerance, stale}; indeterminate, not-decidable and not-evaluated in hues outside both families. Alias `sem.status.<state>` to these values.
>
> In the Rust tooling crate `fid-design`, add `fid-design status-check --root <dir> [--json]` checking completeness, set membership, lock flag and threshold fields. Errors `STATUS-MISSING|STATUS-SET|STATUS-UNLOCKED|STATUS-THRESHOLD|STATUS-SCHEMA`. Exit 0, 1, 2. Tests AC-1..AC-3 of spec node UX.TOKENS.STATUS.
>
> Constraints: no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; no network.

## Open questions

- VIZ.OVERLAY.STATUS's guard uses CIEDE2000 with threshold 20 [unverified]; both values should be read from this file (`uses_artifacts: [UX.TOKENS.STATUS]` on the VIZ side, not editable from here).
- As built: state keys are the semantic token names (`not-decidable`, `not-evaluated`, `out-of-tolerance`), not the build prompt's `not_evaluated`/`out_of_tolerance`, so `sem.status.<state>` and the glyph ID `state.<state>` of UX.ICONS.CLASS use the same word.
- As built: `status.json` has a `cues` key (`{ "<state>": { "glyph": "state.<state>", "text": "icon.state.<state>" } }`) so that no state is carried by colour alone; `status-check` requires it (`STATUS-CUE`). Whether those IDs exist in the icon index is checked once UX.ICONS.CLASS is built.
- As built: two error codes beyond the Design list: `STATUS-ALIAS` (REQ-001: a `sem.status.<state>` that does not resolve to the palette value in a theme, or a `sem.status.*` token that is not a state) and `STATUS-CUE`. A brand-tier entry naming a status token (REQ-005) is reported as `STATUS-SET <name>`.
- `out-of-tolerance` is a nonconforming-set member with no colour in this file: its colours are the out-of-tolerance region colours of UX.TOKENS.DATAPAL, and their distance from CONFORMS is UX.TOKENS.CHECK's to verify.
- AC-4 pending UX.TOKENS.CHECK: until `color-check` exists, REQ-004 is shown by a provisional test (`status::ciede2000` and `status::cvd`, the Machado, Oliveira and Fernandes 2009 model at severity 1.0, matched to the independent oracle `fixtures/ux/tokens/status/make_fixtures.py`). The shipped values clear ΔE2000 20 for every non-CONFORMS state, not only the nonconforming set (smallest nonconforming-set distance 30.05, smallest overall 21.77), because VIZ.OVERLAY.STATUS's build prompt applies the guard to indeterminate, not-decidable and not-evaluated too.
- REQ-002's "hue family" has no numeric definition. The provisional test uses: a colour with OKLCH chroma below 0.03 has no hue; otherwise a family is every hue within 30° of a member [unverified rule]. CONFORMS is azure (OKLCH hue 225), not green: no green CONFORMS could stay ΔE2000 20 from a red NONCONFORMS under protanopia and deuteranopia while meeting text contrast in all four themes.
- UX.TOKENS.COLOR's open question (status values must stay at least 0.10 in OKLab from every claimed role in every theme) is met: `tokens-check` passes with no findings, smallest distance 0.1096. That node's note still calls the `sem.status.*` bindings provisional; it should be closed there (not editable from here).

## Children

_Leaf: none._
