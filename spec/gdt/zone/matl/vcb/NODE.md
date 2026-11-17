---
id: GDT.ZONE.MATL.VCB
title: Virtual-condition boundary containment
level: 4
parent: GDT.ZONE.MATL
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.ZONE.MATL-002, REQ-GDT.ZONE.MATL-003, REQ-GDT.ZONE.MATL-004]
covers: [GOM.GDT.15]
depends_on: [GDT.ZONE.POSE.LIN, GDT.ZONE.POSE.SOLVE, KRN.SOLV.SEQ, KRN.SOLV.CERT]
uses_artifacts: [UNC.ORS.GDT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.ZONE.MATL.VCB: Virtual-condition boundary containment

## Purpose

The surface interpretation of a material-condition tolerance: the toleranced surface must not violate a boundary of virtual-condition size located at the true position (or orientation) in the DRF. This leaf builds the boundary (cylinder for holes and pins, two parallel planes for slots and tabs), and finds the pose within the free DOFs that maximises the minimum clearance between the measured points and the boundary. A non-negative clearance means the surface clears the boundary; the decision itself is UNC's.

## Scope

**In:** boundary size from the chain's combination rule (for example MMC size minus stated tolerance for an internal feature at MMC) and the caller's sizes; boundary geometry `Cylinder` or `TwoPlanes` at the nominal location in the DRF; clearance c_i per point (positive on the material-free side); max-min clearance over the free DOFs as a sequential LP/SOCP; certificate and `local_optimum_only`.
**Out:** bonus arithmetic (owned by `GDT.ZONE.MATL.BONUS`); choosing between interpretations (owned by `GDT.ZONE.MATL.BOTH`); DRF construction (owned by `GDT.DRF.BUILD`).

## Requirements

- **REQ-GDT.ZONE.MATL.VCB-001** The module shall compute the boundary size from the chain's `vc_rule` (one of `mc_minus_tol`, `mc_plus_tol`) applied to the material-condition size and the stated tolerance, and fail with `vc_rule_missing` if the chain names none.
- **REQ-GDT.ZONE.MATL.VCB-002** For an internal feature, the clearance of a point shall be its distance from the boundary axis (or mid-plane) minus the boundary half-size; for an external feature, the boundary half-size minus that distance.
- **REQ-GDT.ZONE.MATL.VCB-003** The module shall return the pose within the free DOFs that maximises the minimum clearance, the minimum clearance in metres (negative when the surface violates the boundary at every pose found), the driving point index and the certificate.
- **REQ-GDT.ZONE.MATL.VCB-004** For internal cylindrical features with free orientation the problem is non-convex; if the certificate is local only, then the result shall carry `local_optimum_only`.
- **REQ-GDT.ZONE.MATL.VCB-005** When the free set is empty, the module shall evaluate clearances at the DRF pose without a solver.
- **REQ-GDT.ZONE.MATL.VCB-006** Results shall reproduce every ORS-GDT position/MMC surface-interpretation case within its stated tolerance.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/zone/matl/hole_vcb.json` (hole Ø10.000 MMC, stated 0.100 mm, boundary Ø9.900 mm at true position, hole surface points on a Ø10.020 mm cylinder offset 0.040 mm from true position, no free DOF), when evaluated, then minimum clearance = (10.020 − 9.900)/2 − 0.040 mm = 0.020 mm within 1e-12 m.
- **AC-2** Given the same hole with free {tx, ty} (datum A only), then the clearance becomes 0.060 mm within 1e-9 m and the pose recovers the 0.040 mm offset.
- **AC-3** Given fixture `slot_vcb.json` (two-plane boundary), then the clearance matches the fixture within 1e-12 m.
- **AC-4** Given a tilted hole with free {rx, ry} and a solver stub reporting a local certificate, then `local_optimum_only`.
- **AC-5** (error path) Given a chain without `vc_rule`, then `vc_rule_missing`.
- **AC-6** (determinism) AC-1 to AC-3 give bit-identical results on all CI platforms.

## Design

- **Registry command(s):** none (internal kernel API).
- **Data structures:** `VcBoundary { Cylinder{axis, diameter_m} | TwoPlanes{mid_plane, width_m} }`; `VcResult { min_clearance_m, driving_index, pose, cert, boundary }`.
- **Algorithm:** maximise t subject to c_i(x) ≥ t for all points, linearised about the current pose with the LIN variable layout; external features give convex cones, internal features give linearised reverse-cone rows solved sequentially by the KRN driver; certify; recompute clearances at the certified pose.
- **Errors:** `vc_rule_missing`, `bad_material_input`, `zone_solve_failed{code}`.
- **Determinism:** D0 for certified results.
- **Performance budget:** 10 000 points, four DOFs < 300 ms [unverified].
- **Crate:** `fid-gps` (module `zone::matl::vcb`).

## Test fixtures

Analytic hole, pin and slot fixtures with planted offsets (CC0, in-repo); ORS-GDT position/MMC cases read as a published artifact.

## Build prompt

> In crate `fid-gps` (kernel, KNP-1), add module `zone::matl::vcb`. Implement `vc_containment(points, kind: Internal|External, boundary_shape: Cylinder|TwoPlanes, nominal_location, mc_size, stated_tol, chain, drf, free) -> Result<VcResult, MatlError>`: boundary size from chain `vc_rule` (`mc_minus_tol` or `mc_plus_tol`; `vc_rule_missing` otherwise); clearance per point = distance to boundary axis or mid-plane minus half-size (internal) or half-size minus distance (external); maximise the minimum clearance over the free DOFs with the sequential solver from `fid-solv` using the `zone::pose::lin` variable layout; no solver when the free set is empty; certify and recompute clearances at the certified pose; carry `local_optimum_only` for local certificates. Tests per the acceptance criteria (hole, slot, free-pose recovery, local flag, error path, bit identity). Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, `fid-num`, fixed-order reductions, total-order min/max, no std `HashMap`/`HashSet`, NaN is an error).

## Open questions

- The `vc_rule` names cover the common combinations; whether resultant-condition boundaries are also needed is for the DMT authors.

## Children

_Leaf: none._
