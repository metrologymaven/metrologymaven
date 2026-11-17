---
id: GDT.DIM.LINEAR
title: Distance and caliper checks
level: 3
parent: GDT.DIM
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.DIM-001, REQ-GDT.DIM-002, REQ-GDT.DIM-003, REQ-GDT.DIM-007]
covers: [GOM.DIM.01, GOM.DIM.02, GOM.DIM.05, PW.GDT.01, PCD.DIM.01]
depends_on: [GDT.CHAR.SPEC, GDT.CHAR.RECORD, GDT.RES.RESOLVE, KRN.CONS]
side_effect: W2
determinism: D0
cli: [fid check eval-distance, fid check eval-caliper]
mcp: [check_eval_distance, check_eval_caliper]
journal: [check.eval_distance@1, check.eval_caliper@1]
updated: 2026-11-16
---

# GDT.DIM.LINEAR: Distance and caliper checks

## Purpose

Evaluates point-to-point and element-to-element distances (3D, projected onto a named plane, or a named axis component of a named frame) and caliper checks (extent of a point set along a direction). These are the "legacy" location and distance dimensions every CMM and scan package offers, and the most common first check users try.

## Scope

**In:** distance between element pairs in the table below; measurand `3d | projected{plane} | component{frame, axis}`; caliper extent along a direction; evaluation from points or fitted features; ResultRecord via the builder; scratch (dry-run) and committed modes.
**Out:** fitting the elements (owned by `KRN`); frames (owned by `ALN`); angles (owned by `GDT.DIM.ANGLE`); diameters (owned by `GDT.DIM.RADIAL`); geometric tolerances (owned by `GDT.FORM`, `GDT.LOC`, `GDT.PROF`).

| Pair | Reference used |
|---|---|
| point–point (circle, sphere centres count as points) | the two points |
| point–line, point–plane | foot of perpendicular from the point |
| line–line | shortest segment between the lines (parallel lines: perpendicular distance) |
| plane–plane | first plane's centroid to the second plane, perpendicular |

## Requirements

- **REQ-GDT.DIM.LINEAR-001** The system shall evaluate distance for every element pair in the Scope table from fitted features, and point-to-point distance from raw points, recording `evaluation_basis` `FEATURES` or `POINTS`.
- **REQ-GDT.DIM.LINEAR-002** If a distance check has no measurand, or names a plane, frame or axis that does not exist, then the command shall fail with `measurand_missing` or `measurand_unknown` and emit no ResultRecord.
- **REQ-GDT.DIM.LINEAR-003** The caliper check shall report max(p·d) − min(p·d) over the selected points for unit direction d, plus the indices of the two extreme points.
- **REQ-GDT.DIM.LINEAR-004** If plane–plane distance is requested for planes whose normals differ by more than 1e-6 rad, then the result shall carry flag `non_parallel_planes` with the angle.
- **REQ-GDT.DIM.LINEAR-005** The value shall be computed in SI metres and reported in the characteristic's authored unit through the single journaled conversion of CONSTITUTION §4.
- **REQ-GDT.DIM.LINEAR-006** Values on the analytic fixtures shall match the expected values within 1e-12 m [unverified: confirm against oracle precision].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/dim/linear_analytic.json` (point pairs, a line pair with known shortest distance, two parallel planes 10 mm apart), when `fid check eval-distance --char <id> --json` runs for each case, then values match `expected/linear_analytic.json` within 1e-12 m and the authored-unit value is the correctly rounded mm decimal.
- **AC-2** Given the same plane pair with measurand `component{frame: part, axis: x}`, then the x component of the reference segment is returned.
- **AC-3** (error path) Given a distance characteristic with no measurand, then the command fails with `measurand_missing` and journals nothing.
- **AC-4** Given fixture `caliper_box.json` (8 corner points of a 20×10×5 mm box, direction x), when `fid check eval-caliper --json` runs, then the value is 20 mm within 1e-12 m and the extreme indices are one -x and one +x corner (lowest index on ties).
- **AC-5** Given two planes 0.01 rad apart, then the plane–plane result carries `non_parallel_planes` with angle 0.01 rad.
- **AC-6** (dry-run) Given `--dry-run`, then a scratch result is returned and the journal is unchanged.
- **AC-7** (replay) Given the journal of AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `check.eval_distance@1` and `check.eval_caliper@1`, W2 (committed to the run, journaled); `--dry-run` gives an R1 scratch result. Params: `char: CharId`, `elements: [ElementRef; 2]` or `points: PointSetRef`, `measurand`, `direction` (caliper). Result: `ResultRecord` with payload `{value_si, reference_segment, flags, extreme_indices}`.
- **Data structures:** `DistanceMeasurand { ThreeD, Projected{plane}, Component{frame, axis} }`.
- **Algorithm:** governing-spec gate → resolve chain (`gdt.resolve_chain@1`) → compute reference segment per the pair table (closed form; line–line via the standard cross-product formula with a parallel fallback) → apply measurand (projection or component) → Euclidean norm with fixed-order summation → builder.
- **Errors:** `measurand_missing`, `measurand_unknown`, `unsupported_pair`, `degenerate_element`, `empty_point_set`, plus pass-through `governing_spec_missing`, `UNRESOLVED`, `UNSUPPORTED`.
- **Determinism:** D0 (closed-form arithmetic under KNP-1).
- **Performance budget:** caliper over 1 M points < 20 ms [unverified].
- **Crate:** `fid-gps` (kernel module; the registry command binding is generated from the manifest).

## Test fixtures

Synthetic analytic cases generated by a seeded script in-repo (CC0), with expected values computed in exact rational arithmetic; ORS-SIZE and RDL cases are added by `UNC.ORS` when available.

## Build prompt

> In crate `fid-gps` (kernel; pure, no I/O), implement `check.eval_distance@1` and `check.eval_caliper@1` (side-effect W2, dry-run R1; CLI `fid check eval-distance`, `fid check eval-caliper`; MCP `check_eval_distance`, `check_eval_caliper`; one journal event each). Pipeline: call the governing-spec gate, resolve the operator chain, compute, then build the ResultRecord with the builder from `fid-gdt` (record DMT row ID, chain digest, DMT digest, `evaluation_basis`). Distances: element pairs point–point, point–line, point–plane, line–line (shortest; parallel fallback), plane–plane (centroid of first to second plane, flag `non_parallel_planes` above 1e-6 rad). Measurand is required: `ThreeD | Projected{plane} | Component{frame, axis}` (`measurand_missing`, `measurand_unknown`). Caliper: max(p·d) − min(p·d) with extreme indices (lowest index on ties). Values in SI metres; reported in the authored unit via the single journaled conversion. Tests: the acceptance criteria above (analytic fixtures within 1e-12 m, error paths journal nothing, dry-run journals nothing, exact replay IDENTICAL). Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`; all input hostile) and KNP-1 (no FMA, transcendentals only through `fid-num`, fixed-order reductions, no std `HashMap`/`HashSet`, total-order min/max, NaN is an error).

## Open questions

- Incumbents differ on plane–plane distance conventions [unverified]; the reference in the Scope table is the default, and other conventions would enter as DMT row variants, not code branches.

## Children

_Leaf: none._
