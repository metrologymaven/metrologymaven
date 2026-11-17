---
id: GDT.LOC.POS.RFS
title: True position and RFS position value of a single feature
level: 4
parent: GDT.LOC.POS
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.LOC.POS-001, REQ-GDT.LOC.POS-004]
covers: [GOM.GDT.08, PW.GDT.04]
depends_on: [GDT.LOC.DERIV.TYPES, GDT.LOC.LRES, GDT.ZONE.ZTYPE, GDT.ZONE.POSE, GDT.DRF.BUILD]
uses_artifacts: [UNC.ORS.GDT]
side_effect: R1
determinism: D3
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.LOC.POS.RFS: True position and RFS position value of a single feature

## Purpose

The core of the P1a product: position of one hole or pin regardless of feature size. The true position is the nominal feature's location and direction expressed in the DRF (from basic dimensions held in the nominal geometry). The derived element (axis or centre point, from `derive()`) is compared with the zone placed at true position: a cylindrical zone for an axis, a spherical or circular zone for a centre point. If the DRF fixes every DoF that moves the zone, the zone usage at the DRF pose is the value; if some DoF is free, the zone-pose solver minimises over it. The value convention (diametral) and zone shape come from the chain.

## Scope

**In:** true-position element from the nominal feature in DRF coordinates; derived element points (axis sampled along its extent at the chain's sample count, or a centre point); zone shape from the chain (cylinder, sphere, circle in a plane); value = zone usage at the DRF pose (no free DoF) or the zone-pose minimum (free DoFs from the DRF's DoF table); per-axis deviations of the actual from the true position in DRF axes through `GDT.LOC.LRES`; driving point index.
**Out:** material condition and bonus (owned by `GDT.LOC.POS.MMC`); patterns (owned by `GDT.LOC.POS.PATTERN`); the command (owned by `GDT.LOC.POS.CMD`); non-cylindrical features (owned by `GDT.LOC.POS.SLOT`); derivation (owned by `GDT.LOC.DERIV`).

## Requirements

- **REQ-GDT.LOC.POS.RFS-001** The function shall place the chain-named zone shape at the true position expressed in the DRF and report the zone usage of the derived element's points as the position value, with the driving point index.
- **REQ-GDT.LOC.POS.RFS-002** When the DRF's DoF table leaves no DoF that moves the zone, the function shall evaluate usage at the DRF pose without a solver; otherwise it shall minimise usage over the free DoFs through the zone-pose solver and report the pose used.
- **REQ-GDT.LOC.POS.RFS-003** The function shall report per-axis deviations (dx, dy, dz in DRF axes) of the derived element's reference point from the true position and the direction deviation for axes.
- **REQ-GDT.LOC.POS.RFS-004** For analytic fixtures the position value shall equal twice the maximum radial distance of the axis sample points from the true-position axis within 1e-12 m, and values shall match every ORS-GDT RFS position case within its stated tolerance.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/loc/pos_hole_offset.json` (hole axis offset (0.03, 0.04) mm from true position, perpendicular, fully constrained DRF A|B|C), when evaluated, then value = 0.100 mm within 1e-12 m, dx = 0.03 mm, dy = 0.04 mm.
- **AC-2** Given the same hole tilted so its top end is offset 0.06 mm and bottom 0 mm, then value = 0.120 mm within 1e-12 m and the driving index is the top sample.
- **AC-3** Given a DRF with only datum A (free in-plane translations and rotation) and a zone-pose stub, then the solver is called with the free DoFs from the DoF table and its pose is reported.
- **AC-4** Given a centre-point feature with a spherical zone, then value = 2 × centre distance from true position.
- **AC-5** (ORS) Every P1a ORS-GDT RFS position case passes when published.
- **AC-6** (determinism) Bit-identical outputs on all CI platforms for AC-1, AC-2, AC-4.

## Design

- **Registry command(s):** none (kernel function `position_rfs(derived, nominal, drf, chain) -> PosValue`).
- **Data structures:** `PosValue { value_m, driving_index, pose_used, deviations: LocDeviation, flags }`.
- **Algorithm:** express nominal in DRF; build `ZoneShape` from the chain; `usage(pose, points)` at the DRF pose or `solve_zone(shape, points, drf, free_dofs)`; deviations via `drf_deviation`.
- **Errors:** `UNSUPPORTED{zone_shape}`, `nan_input`, pass-through derivation and solver errors.
- **Determinism:** D0 at a fixed pose; D3 when the zone-pose solver runs (recorded).
- **Performance budget:** fixed pose < 50 µs per feature [unverified].
- **Crate:** `fid-gps` (module `loc::position::rfs`).

## Test fixtures

Analytic holes and pins with planted offsets and tilts (CC0); ORS-GDT suite from `UNC.ORS.GDT`.

## Build prompt

> In crate `fid-gps` (kernel), module `loc::position::rfs`, implement `position_rfs(derived, nominal, drf, chain) -> Result<PosValue, PosError>`. Express the nominal (true-position) feature in DRF coordinates; build the zone shape the chain names (cylinder, sphere, circle); take the derived element's points (axis samples at the chain's count, or centre point); if the DRF's DoF table leaves no zone-moving DoF, value = `usage(drf_pose, points)`, else `solve_zone(shape, points, drf, free_dofs)` and report the pose; report deviations with `drf_deviation` and the direction deviation; record the driving index. Tests per acceptance criteria (1e-12 m on analytic fixtures). Constraints: CONSTITUTION §8, KNP-1 (no FMA, `fid-num` transcendentals, fixed-order reductions, no std `HashMap`/`HashSet`, NaN is an error).

## Open questions

- None.

## Children

_Leaf: none._
