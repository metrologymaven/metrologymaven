---
id: UX.ICONS.GDT
title: GD&T symbol drawing specifications
level: 3
parent: UX.ICONS
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UX.ICONS-001, REQ-UX.ICONS-005, REQ-UX.ICONS-006]
covers: []
depends_on: [UX.ICONS.GRID, UX.ICONS.PROV]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.ICONS.GDT: GD&T symbol drawing specifications

## Purpose

One canonical geometry for every geometric dimensioning and tolerancing symbol the product draws, so that the viewer, reports and docs render identical shapes. Each symbol is a parametric drawing with all dimensions given as fractions of text height `h`, plus a reference outline that renderers in VIZ and RPT test against. Standards are cited by designator and clause only; shapes are drawn from the project's own measurements of the notation's proportions, not traced.

## Scope

**In:** `gdt/index.json` (`gdt-symbols@1`); one SVG per symbol in units of `h` (`viewBox` height 1000 = 1 h); feature control frame proportions (cell height, padding, separator widths); datum feature and datum target symbol proportions; per-symbol dialect flags (ASME, ISO, both); `fid-design gdt-compare` for consumers.
**Out:** feature control frame layout and rendering in the viewport (VIZ.OVERLAY) and in reports (RPT); which symbols each dialect supports semantically (GDT); symbol font production (an implementation choice of consumers).

## Requirements

- **REQ-UX.ICONS.GDT-001** The index shall contain a drawing for every geometric characteristic symbol and every modifier symbol listed in ASME Y14.5-2018 section 3 and ISO 1101:2017 clause 7 and Annex tables, and for datum feature and datum target symbols per ISO 5459:2011 [unverified clause numbers; confirm at build], each with the designator and clause it comes from.
- **REQ-UX.ICONS.GDT-002** Every drawing shall be expressed in units of text height `h` with stroke width stated as a fraction of `h`, and the feature control frame proportions shall be stated as fractions of `h`.
- **REQ-UX.ICONS.GDT-003** The index shall contain no standards text beyond symbol names in the project's own words, designators and clause numbers.
- **REQ-UX.ICONS.GDT-004** When `fid-design gdt-compare` compares a consumer's rendered outline (SVG or polyline JSON at a given `h`) with the reference, it shall report the maximum Hausdorff distance as a fraction of `h` and fail above 0.005 h [unverified tolerance].
- **REQ-UX.ICONS.GDT-005** Every symbol drawing shall carry a provenance record stating it was drawn from proportions, not traced from a standard's figure or an incumbent's output.

## Acceptance criteria

- **AC-1** Given the shipped `gdt/`, when `fid-design gdt-check --root gdt --json` runs, then it exits 0 and lists every symbol with designator, clause and dialect flags.
- **AC-2** Given the reference `gdt/svg/position.svg` flattened at h = 3.5 mm, when `gdt-compare` compares that outline with the reference, then the distance is 0.
- **AC-3** (error path) Given fixture `fixtures/ux/icons/gdt/fat-circle.json` (circle symbol outline 1 % of h too large), when `gdt-compare` runs, then it exits 1 with `GDT-GEOMETRY circularity 0.010`.
- **AC-4** (error path) Given `no-clause/` (an index entry without a clause), when `gdt-check` runs, then it exits 1 with `GDT-CITE-MISSING`.

## Design

- **Commands:** `fid-design gdt-check --root <gdt dir> [--json]`; `fid-design gdt-compare --symbol <name> --outline <file> --h <length> [--json]`. Tooling, nearest class R0.
- **Format:** `{ name, title_own_words, dialects: [asme, iso], cite: [{ designator, edition, clause }], svg, stroke_h, provenance }`.
- **Comparison:** flatten both outlines to polylines with chord error ≤ 0.0005 h, compute the symmetric Hausdorff distance, divide by `h`.
- **Errors:** `GDT-CITE-MISSING`, `GDT-SCHEMA`, `GDT-SVG`, `GDT-GEOMETRY`, `GDT-UNKNOWN-SYMBOL`.
- **Determinism:** D0 (fixed flattening tolerances, f64 without FMA-dependent paths; reported value rounded to 1e-4 h).
- **Crate:** `fid-design`, module `gdt`.

## Test fixtures

`fixtures/ux/icons/gdt/`: `fat-circle.json`, `no-clause/`; original synthetic data, CC-BY-4.0. No standards figures are used.

## Build prompt

> Draw original SVGs (viewBox height 1000 = one text height h) for every geometric characteristic and modifier symbol in ASME Y14.5-2018 and ISO 1101:2017, plus datum feature and datum target symbols per ISO 5459:2011, from the notation's proportions in your own measurement, not by tracing any figure. Write `gdt/index.json` (`gdt-symbols@1`: name, own-words title, dialects, citations as designator, edition and clause only, svg path, stroke as fraction of h, provenance). Never include standards text.
>
> In the Rust tooling crate `fid-design`, add `fid-design gdt-check --root <dir> [--json]` (schema, citations present, SVG profile per `icons/STYLE.md`) and `fid-design gdt-compare --symbol <name> --outline <svg|json> --h <len> [--json]` (flatten to polylines with chord error ≤ 0.0005 h, symmetric Hausdorff distance divided by h, fail above 0.005). Errors as in spec node UX.ICONS.GDT. Exit 0, 1, 2. Tests AC-1..AC-4.
>
> Constraints: no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded SVG parsing; no network; never store or send standards text (CONSTITUTION §5).

## Open questions

- Whether standard GD&T symbol shapes raise any copyright or trade-dress question is assumed not to (functional notation) [unverified].
- VIZ.OVERLAY and RPT renderers should run `gdt-compare` in their tests (`uses_artifacts: [UX.ICONS.GDT]` on their side); this replaces the font-versus-vector question: either is fine if it passes the comparison.
- The supported-symbol list per dialect comes from GDT; this node draws the full set so that GDT can select.

## Children

_Leaf: none._
