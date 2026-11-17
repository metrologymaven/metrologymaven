---
id: GDT.FORM.STRAIGHT.DERIVED
title: Straightness of a derived median line with optional MMC
level: 4
parent: GDT.FORM.STRAIGHT
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.FORM.STRAIGHT-002, REQ-GDT.FORM.STRAIGHT-003, REQ-GDT.FORM.STRAIGHT-004]
covers: [GOM.GDT.02]
depends_on: [GDT.LOC.DERIV.TYPES, GDT.ZONE.MATL, KRN.MZ, KRN.LSQ]
side_effect: R1
determinism: D3
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.FORM.STRAIGHT.DERIVED: Straightness of a derived median line with optional MMC

## Purpose

Given the derived median line of a cylindrical feature of size (produced by `derive()` with the reduction operator the chain names), reports straightness as the diameter of the smallest cylindrical zone containing it: the minimum-zone 3D line fit's zone diameter, or a least-squares-referenced value flagged as a conservative upper bound while minimum zone is unavailable. With MMC, stated and bonus tolerance come from the material-condition engine. How the median line is derived is not this leaf's concern; it consumes a `DerivedElement::MedianLine`.

## Scope

**In:** calling `derive()` for the median line; zone diameter = 2 × maximum distance of the median-line points to the associated 3D line, with the association named by the chain (minimum zone via `KRN.MZ`, or least squares via `KRN.LSQ`); the conservative fallback policy consistent with `GDT.FORM.FCORE`; MMC through `GDT.ZONE.MATL` (stated tolerance, bonus); payload `{value, reference_line, extreme_index, stated, bonus?, flags}`.
**Out:** median-line derivation (owned by `GDT.LOC.DERIV.MLINE`); bonus and actual size (owned by `GDT.ZONE.MATL` and `GDT.DIM.SIZEASME`); the command (owned by `GDT.FORM.STRAIGHT.CMD`).

## Requirements

- **REQ-GDT.FORM.STRAIGHT.DERIVED-001** The function shall obtain the median line through `derive()` with the chain's reduction operator and report the diameter of the smallest cylindrical zone containing it, computed as twice the maximum distance of its points to the associated 3D line and recomputed from the fit's active set.
- **REQ-GDT.FORM.STRAIGHT.DERIVED-002** While KRN reports minimum zone unavailable for 3D lines, the value shall be computed against the least-squares 3D line and carry `conservative_upper_bound`.
- **REQ-GDT.FORM.STRAIGHT.DERIVED-003** When the characteristic carries MMC, the function shall obtain stated and bonus tolerance from the material-condition engine and report them as separate fields.
- **REQ-GDT.FORM.STRAIGHT.DERIVED-004** If derivation fails, then the function shall pass `derivation_failed{reason}` through and produce no value.
- **REQ-GDT.FORM.STRAIGHT.DERIVED-005** For synthetic median lines with planted deviation, values shall match within 1e-12 m.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/form/median_line_bowed.json` (a median-line stub of 20 points on a circular bow with planted zone diameter 0.010 mm) and a chain naming minimum zone, when evaluated, then value = 0.010 mm within 1e-12 m and the extreme index is the planted point.
- **AC-2** Given a KRN stub with MZ unavailable for 3D lines, then the value is the LS-referenced diameter (>= 0.010 mm) with `conservative_upper_bound`.
- **AC-3** Given an MMC characteristic and a material-condition stub returning bonus 0.004 mm, then the payload has stated 0.010 mm and bonus 0.004 mm as separate fields.
- **AC-4** (error path) Given a `derive()` stub failing with `too_few_sections`, then `derivation_failed{reason: too_few_sections}` and no value.

## Design

- **Registry command(s):** none (kernel function called by `GDT.FORM.STRAIGHT.CMD`).
- **Data structures:** `DerivedStraightnessPayload` as in Scope.
- **Algorithm:** `derive()`; `fit.mz_line3d` or `fit.ls_line3d` as the chain names; distances in index order; total-order max; double.
- **Errors:** pass-through `derivation_failed{reason}`, `UNSUPPORTED{alg}`, material-condition errors.
- **Determinism:** D3 (iterative fits and actual-size envelope fits).
- **Performance budget:** dominated by fits [unverified].
- **Crate:** `fid-gps` (module `form::straightness::derived`).

## Test fixtures

Synthetic median-line stubs with planted bows (CC0).

## Build prompt

> In crate `fid-gps` (kernel), module `form::straightness::derived`, implement `evaluate_derived_straightness(points, chain, krn, matl)`. Call `loc::derive::derive()` with the chain's reduction operator to get a `MedianLine`; associate a 3D line with the KRN fit the chain names; value = 2 x max distance of median-line points to that line (index order, total-order max, recomputed from the active set); while KRN reports MZ unavailable for 3D lines use the least-squares line and set `conservative_upper_bound`. With MMC, call the material-condition engine and report stated and bonus tolerance separately. Pass derivation failures through. Tests per acceptance criteria with stubs. Constraints: CONSTITUTION §8, KNP-1 (no FMA, `fid-num` transcendentals, fixed-order reductions, no std `HashMap`/`HashSet`, NaN is an error).

## Open questions

- Runs end to end only once `GDT.LOC.DERIV.MLINE` (median-line definition, DMT content) and `GDT.ZONE.MATL` exist; tests use stubs until then.

## Children

_Leaf: none._
