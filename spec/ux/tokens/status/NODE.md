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
satisfies: [REQ-UX.TOKENS-009]
covers: []
depends_on: [UX.SYSTEM.PKG]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
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

`fixtures/ux/tokens/status/`: `missing-hc/`, `unlocked/`; synthetic, CC-BY-4.0.

## Build prompt

> Author `tokens/status.json` (`status-palette@1`, `locked: true`, formula `ciede2000`, `min_delta_e2000: 20`) with original colours for CONFORMS, NONCONFORMS, INDETERMINATE, NOT_DECIDABLE, not_evaluated and stale in themes light, dark, hc and floor; conforming set {conforms}; nonconforming set {nonconforms, out_of_tolerance, stale}; indeterminate, not-decidable and not-evaluated in hues outside both families. Alias `sem.status.<state>` to these values.
>
> In the Rust tooling crate `fid-design`, add `fid-design status-check --root <dir> [--json]` checking completeness, set membership, lock flag and threshold fields. Errors `STATUS-MISSING|STATUS-SET|STATUS-UNLOCKED|STATUS-THRESHOLD|STATUS-SCHEMA`. Exit 0, 1, 2. Tests AC-1..AC-3 of spec node UX.TOKENS.STATUS.
>
> Constraints: no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; no network.

## Open questions

- VIZ.OVERLAY.STATUS's guard uses CIEDE2000 with threshold 20 [unverified]; both values should be read from this file (`uses_artifacts: [UX.TOKENS.STATUS]` on the VIZ side, not editable from here).

## Children

_Leaf: none._
