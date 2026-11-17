---
id: GDT.PROF.SURF.PROB
title: Single-segment surface-profile problem
level: 4
parent: GDT.PROF.SURF
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.PROF.SURF-001, REQ-GDT.PROF.SURF-004]
covers: [GOM.GDT.12]
depends_on: [GDT.PROF.DEV, GDT.PROF.ZDEF.UNIF, GDT.PROF.BESTFIT.OPTS, GDT.ZONE.POSE, GDT.DRF.BUILD, GEO.CAD.GROUP]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.PROF.SURF.PROB: Single-segment surface-profile problem

## Purpose

Builds and solves the zone problem for one profile-of-a-surface segment: the toleranced face set, the points that project onto it, the zone from ZDEF, the DOFs the DRF leaves free (all six with no datums, none with a full DRF), adjusted by the best-fit option, and the call into `GDT.ZONE.POSE`. It returns the solved segment (value, extremes, pose, certificate) that the command and the composite builder both use.

## Scope

**In:** toleranced face set from face IDs or a CAD group; point-to-face restriction through DEV; `ProfileZoneProblem` construction; baseline free DOFs from the DRF; best-fit adjustment through `GDT.PROF.BESTFIT.OPTS`; solve through `GDT.ZONE.POSE` with the offset shape and a re-projection oracle; segment result.
**Out:** tier construction (owned by `GDT.PROF.SURF.COMP`); the command and ResultRecord (owned by `GDT.PROF.SURF.CMD`); mobility search (owned by `GDT.ZONE.MOBSRCH`, reached through the best-fit option); least-squares objective (owned by `GDT.PROF.BESTFIT.LSQ`).

## Requirements

- **REQ-GDT.PROF.SURF.PROB-001** The module shall take the toleranced face set from the characteristic as face IDs or as one CAD group ID, expand a group to its face members through `GEO.CAD.GROUP`, and fail with `face_set_empty` if the set is empty.
- **REQ-GDT.PROF.SURF.PROB-002** If the characteristic states an extent that ends part-way across a face, then the module shall return `UNSUPPORTED{partial_face_extent}`.
- **REQ-GDT.PROF.SURF.PROB-003** The baseline free DOFs shall be the DRF's free DOFs when the characteristic has datums and all six DOFs when it has none, then adjusted by `pose_dofs` from `GDT.PROF.BESTFIT.OPTS`.
- **REQ-GDT.PROF.SURF.PROB-004** The module shall solve the segment with `ZoneShape::Offset{shift}` from ZDEF over the adjusted DOFs, passing a re-projection oracle that recomputes deviations through DEV at each trial pose.
- **REQ-GDT.PROF.SURF.PROB-005** The segment result shall contain the value (2·max|d_i − shift| at the returned pose), signed minimum and maximum deviation with their point indices, the pose, the free DOFs used, the maximum approximation error over faces hit, the count of `no_projection` points, and the solver certificate or `local_optimum_only`.
- **REQ-GDT.PROF.SURF.PROB-006** If fewer than the chain's `min_points` (default 1) points project onto the face set, then the module shall return `too_few_points`.
- **REQ-GDT.PROF.SURF.PROB-007** Segment values shall match every ORS-GDT surface-profile and ORS-PROJ analytic case within its stated tolerance.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/prof/surf/sphere_fixed.json` (sphere r = 20 mm, full DRF, planted radial deviations in [−0.03, +0.05] mm, bilateral t = 0.2 mm), when the segment is solved, then no solver is called, value = 0.10 mm within 1e-12 m, and min/max = −0.03/+0.05 mm with the planted indices.
- **AC-2** Given fixture `plane_cyl_free.json` (no datums; points displaced by a known rigid transform plus planted form deviations of range 0.02 mm), then the value equals the planted minimum-zone value within 1e-9 m and the free DOFs are all six.
- **AC-3** Given a partial DRF that frees `tz` only, then the solved pose differs from the DRF pose only in `tz`.
- **AC-4** Given a CAD group of three faces, then the face set equals the group's members and points projecting elsewhere are reported `no_projection`.
- **AC-5** (error path) Given an extent `between` two points on one face, then `UNSUPPORTED{partial_face_extent}`; given an empty group, then `face_set_empty`.
- **AC-6** (determinism) AC-1 and AC-2 give bit-identical results on all CI platforms.

## Design

- **Registry command(s):** none (library for `check.eval_surface_profile@1` and the composite builder).
- **Data structures:** `ProfileZoneProblem { points, faces: FaceSet, drf: Option<Drf>, zone: ZoneIntervals, free: DofSet, bestfit: BestFitRecord }`; `SegmentResult { value_m, dev_min, dev_max, pose, free, max_approx_error_m, no_projection: u32, cert: CertOrFlag }`.
- **Algorithm:** resolve face set; deviations at the DRF pose (the CAD pose with no datums; a least-squares start, when the best-fit option asks for one, is supplied by the caller); compute free DOFs; call `zone::solve_zone(Offset{shift}, ..., free, oracle)`; recompute deviations and value at the returned pose.
- **Errors:** `face_set_empty`, `UNSUPPORTED{partial_face_extent}`, `too_few_points`, pass-through from DEV, ZDEF and the zone solver.
- **Determinism:** D0 when the pose is fixed or the solver certifies; otherwise the certificate's flag is carried.
- **Performance budget:** 1 M points, 200 faces, six free DOFs < 5 s [unverified].
- **Crate:** `fid-gps` (module `prof::surf::problem`).

## Test fixtures

Analytic sphere, plane and cylinder fixtures with planted deviations and rigid displacements (CC0, in-repo); one CAD-group fixture; ORS-PROJ analytic cases when published.

## Build prompt

> In crate `fid-gps` (kernel, KNP-1), add module `prof::surf::problem`. Define `ProfileZoneProblem` and `SegmentResult` (value, signed min/max with indices, pose, free DOFs, max approximation error, `no_projection` count, certificate or `local_optimum_only`). Implement `solve_segment(char, points, drf, chain, bestfit) -> Result<SegmentResult, ProfError>`: face set from face IDs or one CAD group (expanded via the GEO group API; `face_set_empty` if empty); `UNSUPPORTED{partial_face_extent}` for extents ending inside a face; baseline free DOFs = DRF free DOFs (all six with no datums), adjusted by `prof::bestfit::pose_dofs`; zone from `prof::zdef::zone_intervals`; solve with `zone::solve_zone(ZoneShape::Offset{shift}, …)` and a re-projection oracle over `prof::dev::deviations`; recompute value 2·max|d − shift| and extremes at the returned pose; `too_few_points` below the chain's minimum. Tests per the acceptance criteria. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, `fid-num`, fixed-order reductions, total-order min/max, no std `HashMap`/`HashSet`, NaN is an error).

## Open questions

- Whether a datum-less profile of a surface should start from a least-squares pose or from the CAD pose is a numerical choice for the solver's start, not interpretation; the minimum-zone result is the same when the solver certifies.

## Children

_Leaf: none._
