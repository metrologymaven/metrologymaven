---
id: GDT.ZONE.POSE.SOLVE
title: Zone-pose solve driver and certificates
level: 4
parent: GDT.ZONE.POSE
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.ZONE.POSE-001, REQ-GDT.ZONE.POSE-003, REQ-GDT.ZONE.POSE-004, REQ-GDT.ZONE.POSE-005]
covers: []
depends_on: [GDT.ZONE.POSE.LIN, GDT.ZONE.POSE.FIXED, GDT.ZONE.ZTYPE, KRN.SOLV.SEQ, KRN.SOLV.CERT]
uses_artifacts: [UNC.ORS.GDT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.ZONE.POSE.SOLVE: Zone-pose solve driver and certificates

## Purpose

The public `solve_zone` entry point. It routes an empty free-DOF set to the fixed path, otherwise runs the KRN sequential solver on the LIN problem from the DRF pose (or a caller start), certifies the active set, recomputes the value with `GDT.ZONE.ZTYPE` at the certified pose, and packages a `ZoneSolution` with the certificate or the `local_optimum_only` flag. Every evaluating family calls this one function, so the "recomputed, never iterated" rule holds everywhere.

## Scope

**In:** `solve_zone(shape, points, drf, free, extra, start, oracle) -> ZoneSolution`; routing to FIXED; solver configuration (fixed iteration limits and tolerances exported for the kernel manifest); certificate handling; value recomputation; `local_optimum_only` propagation; algorithm IDs `gps.zone.<method>@x.y.z`.
**Out:** problem rows (owned by `GDT.ZONE.POSE.LIN`); infeasibility listing at a stated size (owned by `GDT.ZONE.POSE.INFEAS`); solver internals (owned by `KRN.SOLV`).

## Requirements

- **REQ-GDT.ZONE.POSE.SOLVE-001** When the free set is empty and no extra variables are supplied, `solve_zone` shall delegate to `GDT.ZONE.POSE.FIXED` and call no solver.
- **REQ-GDT.ZONE.POSE.SOLVE-002** Otherwise it shall run the KRN sequential driver on the LIN problem from the start pose (the DRF pose unless the caller supplies one), with iteration limits and tolerances fixed in code and recorded in the algorithm ID's parameters.
- **REQ-GDT.ZONE.POSE.SOLVE-003** The reported usage shall be `GDT.ZONE.ZTYPE` usage at the certified pose, never the solver objective, and the driving index shall be ZTYPE's.
- **REQ-GDT.ZONE.POSE.SOLVE-004** If the certificate carries `local_optimum_only` (non-convex shape with free orientation), then the solution's certificate shall be `LocalOnly` and the flag shall reach the caller unchanged.
- **REQ-GDT.ZONE.POSE.SOLVE-005** If the solver returns an error (iteration limit, infeasible subproblem, certificate failure), then `solve_zone` shall return `zone_solve_failed` with the solver's code and no value.
- **REQ-GDT.ZONE.POSE.SOLVE-006** The solution shall carry the algorithm ID `gps.zone.<shape>_mz@<semver>`, the free DOF set and the start-pose digest.
- **REQ-GDT.ZONE.POSE.SOLVE-007** The solver shall reproduce every ORS-GDT orientation and RFS position case within its stated tolerance.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/zone/orient_parallel.json` (parallelism of a plane to datum A, free {tz}, planted width 0.01 mm), when `solve_zone` runs, then usage = 0.01 mm within 1e-12 m and the certificate is `Certified`.
- **AC-2** Given fixture `pos_rfs_hole.json` (hole axis to A|B|C, cylinder zone, free none), then FIXED is used and no solver is called (stub counts zero calls).
- **AC-3** Given fixture `perp_axis_free.json` (perpendicularity of an axis, cylinder zone, free {rx, ry} on a non-convex arrangement), then the result is `LocalOnly` and the flag appears in the solution.
- **AC-4** Given two different start poses reaching the same active set (seeded), then usage and pose are bit-identical.
- **AC-5** (error path) Given a solver stub returning iteration-limit, then `zone_solve_failed{iteration_limit}` and no value.
- **AC-6** When ORS-GDT orientation and RFS position cases are published, every case passes within its stated tolerance.

## Design

- **Registry command(s):** none (internal kernel API).
- **Data structures:** `ZoneSolution` (from FIXED); `SolveConfig` (constants, exported to the manifest).
- **Algorithm:** route; build `ZoneLinProblem`; `seq::solve`; `cert::certify`; transform points by the certified pose; ZTYPE usage; package.
- **Errors:** `zone_solve_failed{code}`, pass-through `ZoneError`.
- **Determinism:** D0 for certified results (same active set gives bit-identical values per `KRN.SOLV.CERT`); `LocalOnly` results are still D0 for a given start.
- **Performance budget:** 10 000 points, six DOFs, plane zone < 200 ms [unverified].
- **Crate:** `fid-gps` (module `zone::pose`).

## Test fixtures

Analytic orientation and position fixtures with planted widths (CC0, in-repo); solver stubs for error paths; ORS-GDT cases read as a published artifact.

## Build prompt

> In crate `fid-gps` (kernel, KNP-1), module `zone::pose`, implement `solve_zone(shape, points, drf, free: DofSet, extra: ExtraConstraints, start: Option<Pose>, oracle) -> Result<ZoneSolution, ZoneError>`. Empty free set and no extra variables → `evaluate_fixed` with no solver call. Otherwise run the sequential driver from `fid-solv` on `zone::pose::lin::ZoneLinProblem` from the start pose (DRF pose by default) with fixed limits, certify the active set with `fid-solv` cert, transform points by the certified pose and take the value and driving index from `ZoneShape::usage` (never the solver objective). Map certificate flag `local_optimum_only` to `ZoneCert::LocalOnly`; map solver errors to `zone_solve_failed{code}`. Record algorithm ID `gps.zone.<shape>_mz@<semver>`, free set and start-pose digest. Tests per the acceptance criteria (planted widths, no-solver path, local-only flag, start-independence, error stub). Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, `fid-num`, fixed-order reductions, total-order min/max, no std `HashMap`/`HashSet`, seeded RNG only, NaN is an error).

## Open questions

- Whether multi-start (`KRN.SOLV.MSTART`) should be used by default for non-convex shapes, to reduce `LocalOnly` results, is a performance and reproducibility trade-off for the KRN maintainers.

## Children

_Leaf: none._
