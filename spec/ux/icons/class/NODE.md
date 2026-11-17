---
id: UX.ICONS.CLASS
title: Side-effect class icons, status glyphs and provenance badges
level: 3
parent: UX.ICONS
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-UX.ICONS-001, REQ-UX.ICONS-003, REQ-UX.ICONS-004, REQ-UX.ICONS-007]
covers: []
depends_on: [UX.ICONS.GRID, UX.ICONS.PROV]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.ICONS.CLASS: Side-effect class icons, status glyphs and provenance badges

## Purpose

The meaning-carrying glyphs: one icon for each of the eleven side-effect classes, one glyph for each of the six display states, one badge shape for each of the five provenances, and the stripe pattern for out-of-tolerance regions. They must be told apart by shape alone, in greyscale, at 16 px, because they carry safety and conformance meaning. A shape-distance check backs the human review.

## Scope

**In:** icons `class.r0` … `class.a0`; glyphs `state.conforms`, `state.nonconforms`, `state.indeterminate`, `state.not-decidable`, `state.not-evaluated`, `state.stale`; badges `prov.computed`, `prov.measured`, `prov.entered`, `prov.reported`, `prov.suggested`; the out-of-tolerance stripe pattern; `fid-design glyph-distinct`.
**Out:** state-to-glyph mapping at runtime (`VIZ.OVERLAY.STATUS`); status colours (`UX.TOKENS.STATUS`); where approval chips show class icons (`UX.COMP.CMD`, `UX.PATTERN.PROPOSE`).

## Requirements

- **REQ-UX.ICONS.CLASS-001** The set shall contain exactly one icon per side-effect class of CONSTITUTION §3, one glyph per display state and one badge per provenance, each with an index entry and an accessible-name message ID.
- **REQ-UX.ICONS.CLASS-002** When `fid-design glyph-distinct` rasterises each group (classes, states, badges) at 16 px greyscale with 4× supersampling, every pair within a group shall differ in at least 20 % of covered pixels [unverified threshold] after best alignment within ±1 px.
- **REQ-UX.ICONS.CLASS-003** The M1, S1, W3 and A0 class icons shall share a common outer shape that no other class icon uses, so that human-only and never-on-MCP actions are recognisable as a family.
- **REQ-UX.ICONS.CLASS-004** The stale and not-evaluated glyphs shall share no outer shape with the conforms or nonconforms glyphs.

## Acceptance criteria

- **AC-1** Given the shipped glyphs, when `fid-design glyph-distinct --root icons --json` runs, then it exits 0 and reports the minimum pairwise difference per group.
- **AC-2** (error path) Given fixture `fixtures/ux/icons/class/twins/` (two state glyphs differing by one pixel), when the check runs, then it exits 1 with `GLYPH-SIMILAR state.indeterminate state.not-decidable`.
- **AC-3** (error path) Given `missing-class/` (no `class.i1`), when the check runs, then it exits 1 with `GLYPH-MISSING class.i1`.
- **AC-4** Given the design-review record, then a human reviewer confirms greyscale distinctness at 16 px in all four themes and the shape-family rule of REQ-003.

## Design

- **Command:** `fid-design glyph-distinct --root <icons dir> [--threshold 0.20] [--json]`. Tooling, nearest class R0.
- **Rasteriser:** a pure-Rust SVG rasteriser on the restricted profile of `UX.ICONS.GRID`, anti-aliasing off at 4× then box-downsampled; coverage threshold 0.5.
- **Metric:** symmetric difference of covered pixels divided by union, minimised over integer shifts in −1..1 px.
- **Errors:** `GLYPH-MISSING`, `GLYPH-SIMILAR`, `GLYPH-FAMILY`, `GLYPH-RASTER`.
- **Determinism:** D0 (integer coverage after fixed supersampling).
- **Crate:** `fid-design`, module `glyph`.

## Test fixtures

`fixtures/ux/icons/class/`: `twins/`, `missing-class/`; original synthetic drawings, CC-BY-4.0.

## Build prompt

> Draw original glyphs following `icons/STYLE.md`: eleven side-effect class icons `class.r0, r1, w1, w2, w3, x1, i1, d1, m1, s1, a0` (M1, S1, W3, A0 share an outer shape no other class uses), six display-state glyphs, five provenance badges and the out-of-tolerance stripe pattern, with index entries and `icon.*` accessible names. In the Rust tooling crate `fid-design`, add `fid-design glyph-distinct --root <dir> [--threshold] [--json]`: rasterise each glyph at 16 px greyscale with 4× supersampling, compute the symmetric-difference-over-union with best shift in −1..1 px for each pair in a group, fail below the threshold (default 0.20). Errors `GLYPH-MISSING|GLYPH-SIMILAR|GLYPH-FAMILY|GLYPH-RASTER`. Exit 0, 1, 2. Tests AC-1..AC-3 of spec node UX.ICONS.CLASS.
>
> Constraints: no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; integer arithmetic for coverage; no network.

## Open questions

- VIZ.OVERLAY.STATUS names `IconId` and `PatternId` per state; it should take them from this node's index (`uses_artifacts`, not editable from here).

## Children

_Leaf: none._
