---
id: GDT.FORM.ROUND
title: Roundness (circularity) with cross-section extraction
level: 3
parent: GDT.FORM
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.FORM-001, REQ-GDT.FORM-005]
covers: [GOM.GDT.03, PW.GDT.02]
depends_on: [GDT.FORM.FCORE, GDT.CHAR.SPEC, KRN.LSQ]
side_effect: W2
determinism: D0
cli: [fid check eval-roundness]
mcp: [check_eval_roundness]
journal: [check.eval_roundness@1]
built_in: [fid-gps, fid-gdt]
updated: 2026-11-23
---

# GDT.FORM.ROUND: Roundness (circularity) with cross-section extraction

## Purpose

Evaluates roundness of circular cross-sections: per section, the radial width of the narrowest pair of concentric circles containing the section's points (minimum zone) or the least-squares-referenced radial range. Points come either as explicit 2D sections (typical for tactile scanning) or as a 3D set from which sections are cut perpendicular to a reference axis at the positions and band width the chain names.

## Scope

**In:** `check.eval_roundness@1`; explicit sections; section extraction from 3D points (reference axis from the KRN least-squares cylinder or cone fit named by the chain, section positions, band half-width, projection onto the section plane); per-section form via `GDT.FORM.FCORE` with element `Circle2d`; reporting the worst section and all section values.
**Out:** form policy (owned by `GDT.FORM.FCORE`); cylindricity (owned by `GDT.FORM.CYL`); filters (owned by `KRN`).

## Requirements

- **REQ-GDT.FORM.ROUND-001** For each section, the command shall compute roundness through the form core with element `Circle2d` and report section value, reference circle and peak and valley indices.
- **REQ-GDT.FORM.ROUND-002** The characteristic value shall be the maximum section value, and the result shall list every section's position and value.
- **REQ-GDT.FORM.ROUND-003** When sections are extracted from a 3D set, the command shall select points within the chain's band half-width of each section plane, project them onto it, and record axis, positions and band width.
- **REQ-GDT.FORM.ROUND-004** If a section holds fewer points than the row minimum, then the command shall fail with `too_few_points` naming the section.
- **REQ-GDT.FORM.ROUND-005** Minimum-zone roundness shall match the synthetic fixtures within 1e-12 m and, when run, the TraCIM Chebyshev 2D circle test within its stated MPE [unverified].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/form/round_known_mz.json` (three explicit sections with planted lobing giving 0.004, 0.006 and 0.005 mm), when `fid check eval-roundness --json` runs, then the value is 0.006 mm within 1e-12 m and all three section values are listed.
- **AC-2** Given fixture `round_from_cylinder_cloud.json` (seeded 3D points on a cylinder with known per-height lobing) and a chain naming sections at z = 5, 10, 15 mm with band half-width 0.1 mm, then section values match the planted values within 1e-9 m and the record lists axis, positions and band.
- **AC-3** (error path) Given a section with 2 points, then `too_few_points{section: 1}` and nothing journaled.
- **AC-4** (replay) Exact replay of AC-1 gives `IDENTICAL`.
- **AC-5** (optional, external) TraCIM Chebyshev circle data passes within its MPE [unverified].

## Design

- **Registry command(s):** `check.eval_roundness@1`, W2, dry-run R1. Params: `char`, `sections: [SectionPoints]` or `points: PointSetRef`. Result: ResultRecord with `FormPayload` per section plus `sections: [{position, value}]`.
- **Data structures:** `SectionPoints { plane, points2d }`; `SectionSpec { axis_fit, positions_m, band_half_width_m }` from the chain.
- **Algorithm:** gate → resolve → (extract: LS axis fit, band selection in index order, projection) → per section `evaluate_form(Circle2d, …)` → max over sections (total order, lowest section index on ties) → builder.
- **Errors:** `too_few_points{section}`, `section_spec_missing` (3D input but chain has no section spec), pass-through errors.
- **Determinism:** D0.
- **Performance budget:** 100 sections × 3 600 points < 300 ms [unverified].
- **Crate:** `fid-gps`.

## Test fixtures

Seeded synthetic sections and cylinder clouds with planted lobing (CC0, in-repo); TraCIM data through the service only.

## Build prompt

> In crate `fid-gps` (kernel), implement `check.eval_roundness@1` (W2, dry-run R1; CLI `fid check eval-roundness`; MCP `check_eval_roundness`; one journal event). Accept explicit 2D sections or a 3D point set; for 3D input take the section spec (axis fit, positions, band half-width) from the resolved chain (`section_spec_missing` if absent), fit the reference axis with the named KRN least-squares fit, select points within the band of each section plane in index order and project them. For each section call `form::evaluate_form(FormElement::Circle2d, …)`; the characteristic value is the maximum section value (lowest section index on ties); list every section's position and value. Fail `too_few_points{section}` per section. Build the ResultRecord with `fid-gdt`'s builder. Tests per the acceptance criteria. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`; all input hostile) and KNP-1 (no FMA, transcendentals only through `fid-num`, fixed-order reductions, no std `HashMap`/`HashSet`, total-order min/max, NaN is an error).

## Open questions

- Default section count and band width when the drawing is silent are DMT content (D-009); the fixtures use explicit values.

## Children

_Leaf: none._
