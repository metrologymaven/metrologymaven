---
id: GDT.LOC.DERIV.AXIS
title: Axis of an associated feature
level: 4
parent: GDT.LOC.DERIV
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.LOC.DERIV-001, REQ-GDT.LOC.DERIV-003]
covers: []
depends_on: [GDT.LOC.DERIV.TYPES, KRN.LSQ, KRN.ENV, KRN.MZ]
uses_artifacts: [UNC.ORS.GDT]
side_effect: R1
determinism: D3
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.LOC.DERIV.AXIS: Axis of an associated feature

## Purpose

The simplest derived element: associate a cylinder or cone to the feature's points with the fit the chain names (least squares, minimum zone, maximum inscribed or minimum circumscribed) and take its axis, bounded by the extent of the feature along it. It is pure association plus geometry; which association a callout uses is the row's choice. Orientation and position of holes and pins in P1a run on this element.

## Scope

**In:** cylinder and cone axes from the chain-named KRN fit; axis extent as the minimum and maximum projection of the feature points onto the axis; `Axis{point (at the extent midpoint), direction (oriented to the nominal direction's hemisphere), extent}`; registration in the `derive()` registry; flags (`local_optimum_only`, trust-region errors) passed through.
**Out:** median lines from section centres (owned by `GDT.LOC.DERIV.MLINE`); median surfaces and points (owned by `GDT.LOC.DERIV.MSURF`); fits (owned by `KRN`).

## Requirements

- **REQ-GDT.LOC.DERIV.AXIS-001** The function shall associate the feature with exactly the KRN fit the chain names and return the fit's axis with that fit's algorithm ID.
- **REQ-GDT.LOC.DERIV.AXIS-002** The axis extent shall be the minimum and maximum of the projections of all feature points onto the axis, computed in index order, and the axis point shall be the midpoint of that extent.
- **REQ-GDT.LOC.DERIV.AXIS-003** The axis direction shall be oriented so its dot product with the nominal axis direction is non-negative.
- **REQ-GDT.LOC.DERIV.AXIS-004** If the fit fails or flags `local_optimum_only`, then the function shall return `derivation_failed{reason: fit_failed}` or carry the flag, respectively.
- **REQ-GDT.LOC.DERIV.AXIS-005** For noise-free analytic cylinders and cones, the axis shall match the generating axis within 1e-12 m and 1e-12 rad, and axes shall match every ORS-GDT derived-axis case within its stated tolerance.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/loc/axis_tilted_cyl.json` (cylinder, axis tilted 0.01 rad, length 30 mm, exact points), when derived with the least-squares fit, then direction and point match within 1e-12 rad and 1e-12 m and the extent is 30 mm.
- **AC-2** Given the same with a nominal direction pointing the other way, then the returned direction flips accordingly.
- **AC-3** Given fixture `axis_cone.json`, then the cone axis matches within 1e-12.
- **AC-4** Given a KRN stub flagging `local_optimum_only`, then the element carries the flag.
- **AC-5** (ORS) Every ORS-GDT derived-axis case passes when published.

## Design

- **Registry command(s):** none (registered implementation of `derive()`).
- **Data structures:** reuses `DerivedElement::Axis`.
- **Algorithm:** fit; projections `t_i = d·(p_i − c)` in index order; total-order min and max; midpoint.
- **Errors:** `derivation_failed{reason: fit_failed}`, `UNSUPPORTED{alg}` for a non-axial fit.
- **Determinism:** D3 (iterative axial fits).
- **Performance budget:** dominated by the fit [unverified].
- **Crate:** `fid-gps` (module `loc::derive::axis`).

## Test fixtures

Analytic cylinders and cones (CC0); ORS-GDT suite from `UNC.ORS.GDT`.

## Build prompt

> In crate `fid-gps`, module `loc::derive::axis`, implement the axis derivation registered in the `derive()` registry: call exactly the KRN cylinder or cone fit the chain names, take its axis, orient it to the nominal direction's hemisphere, compute extent as min and max projection of all points (index order, total-order min/max) and set the axis point at the extent midpoint; propagate `local_optimum_only`; map fit failure to `derivation_failed{reason: fit_failed}`. Tests per acceptance criteria (1e-12 on analytic data). Constraints: CONSTITUTION §8, KNP-1 (no FMA, `fid-num` transcendentals, fixed-order reductions, no std `HashMap`/`HashSet`, NaN is an error).

## Open questions

- None.

## Children

_Leaf: none._
