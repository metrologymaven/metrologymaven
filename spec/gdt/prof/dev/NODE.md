---
id: GDT.PROF.DEV
title: Signed deviations from nominal with approximation error
level: 3
parent: GDT.PROF
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.PROF-005, REQ-GDT.PROF-006]
covers: []
depends_on: [GEO.DIST, GEO.CAD]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GDT.PROF.DEV: Signed deviations from nominal with approximation error

## Purpose

Turns measured points into the per-point signed deviations that profile and runout zones consume: closest-point projection onto the nominal faces (from GEO), the signed distance with material side positive, the face ID hit, and the face's `nominal_approximation_error` from STEP-to-QIF conversion, so UNC can include it as a contributor. It is the profile family's equivalent of the form core.

## Scope

**In:** `Deviation { index, face_id, foot_point, normal, signed_dev, approx_error }` for each point at a given pose; restriction to the toleranced faces; maximum approximation error over the faces hit; flags for points whose foot lies on a face boundary or edge.
**Out:** projection itself (owned by `GEO.DIST`); nominal geometry and the per-face error values (owned by `GEO.CAD` and `IO.CAD`); zone definitions and evaluation (owned by `GDT.PROF.ZDEF`, `GDT.PROF.LINE`, `GDT.PROF.SURF`, `GDT.PROF.RUNOUT`).

## Requirements

- **REQ-GDT.PROF.DEV-001** For each point, the system shall return the closest point on the toleranced faces, the face ID, the outward unit normal there, and the signed deviation with the material side positive.
- **REQ-GDT.PROF.DEV-002** Each deviation shall carry the hit face's `nominal_approximation_error` (0 for exact analytic faces, never omitted), and the result shall carry the maximum over all faces hit.
- **REQ-GDT.PROF.DEV-003** If a foot point lies on a face boundary within 1e-9 m, then the deviation shall carry flag `on_boundary`.
- **REQ-GDT.PROF.DEV-004** If a point is farther than a caller-given search radius from every toleranced face, then it shall be returned with flag `no_projection` and excluded from zone evaluation by the caller.
- **REQ-GDT.PROF.DEV-005** Deviations shall match ORS-PROJ cases for analytic faces within each case's stated tolerance.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/prof/dev_sphere_plane.json` (exact sphere r = 20 mm and plane faces, points at known offsets ±0.05 mm), when deviations are computed, then signed values match within 1e-12 m and approximation errors are 0.
- **AC-2** Given fixture `dev_nurbs_approx.json` (one face carrying `nominal_approximation_error` 2e-7 m), then every deviation on that face carries 2e-7 m and the result maximum is 2e-7 m.
- **AC-3** Given a point whose foot lies on the shared edge of two faces, then `on_boundary` is set and the lower face ID is reported.
- **AC-4** Given a point 10 mm away with search radius 1 mm, then `no_projection`.
- **AC-5** (determinism) 100 000 seeded points give bit-identical output on all CI platforms.

## Design

- **Registry command(s):** none (library for the profile and runout commands).
- **Data structures:** `Deviation` as in Scope; `DeviationSet { items, max_approx_error, pose_digest }`.
- **Algorithm:** transform points by the pose; call GEO's closest-point query restricted to the toleranced face set; orient normal outward from material; sign = sign of (p − foot)·n; ties on equal distance resolved to the lowest face ID.
- **Errors:** `face_unknown`, `nan_input`, pass-through projection errors.
- **Determinism:** D0 (requires GEO projection D0 on analytic faces).
- **Performance budget:** 1 M points against 100 analytic faces < 1 s [unverified].
- **Crate:** `fid-gps` (module `prof::dev`).

## Test fixtures

Analytic sphere, plane and cylinder faces with seeded points (CC0, in-repo); one synthetic face with a declared approximation error; ORS-PROJ cases when published.

## Build prompt

> In crate `fid-gps` (kernel, pure, KNP-1), module `prof::dev`, implement `deviations(points, pose, faces: &FaceSet, search_radius) -> Result<DeviationSet, DevError>` using GEO's closest-point projection restricted to the toleranced faces. For each point return face ID, foot point, outward unit normal, signed deviation (material side positive), the face's `nominal_approximation_error` (0 for exact faces; never omitted), and flags `on_boundary` (foot within 1e-9 m of a face boundary; report the lowest face ID on ties) and `no_projection` (beyond the search radius). Carry the maximum approximation error over faces hit and the pose digest. Tests per the acceptance criteria. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, `fid-num`, fixed-order reductions, total-order min/max, no std `HashMap`/`HashSet`, NaN is an error).

## Open questions

- Trimmed-NURBS projection determinism depends on `GEO.DIST` (WP-PROJ); until it is D0 on NURBS, results on NURBS faces may need determinism class D3 and a flag.

## Children

_Leaf: none._
