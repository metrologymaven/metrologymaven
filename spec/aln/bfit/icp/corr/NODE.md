---
id: ALN.BFIT.ICP.CORR
title: Surface best-fit correspondence search and tie-break
level: 4
parent: ALN.BFIT.ICP
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-align-solve]
satisfies: [REQ-ALN.BFIT.ICP-001, REQ-ALN.BFIT.ICP-003, REQ-ALN.BFIT.ICP-005, REQ-ALN.BFIT.ICP-006]
covers: []
depends_on: [GEO.DIST, GEO.NURBS.EVAL, GEO.TYPES.MESH]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ALN.BFIT.ICP.CORR: Surface best-fit correspondence search and tie-break

## Purpose

For each measured point (already moved by the current pose), finds its closest point on the reference mesh or CAD body, the surface normal there and the stable ID of the reference entity, through GEO's deterministic batch projection. It is the only place the ICP loop touches the reference.

## Scope

**In:** batch closest-point query against a `ProjectionTarget` (mesh or CAD body); restriction to the reference patches of a local scope; optional maximum correspondence distance; normal at the foot (face normal, GEO.DIST.SIGN pseudo-normal at edges and vertices, NURBS normal for CAD); verification of the lowest-ID tie-break.
**Out:** projection and tie-break arithmetic (`GEO.DIST`); spatial indexes (`GEO.INDEX`); residuals (`ALN.BFIT.ICP.METRIC`); scope resolution (`ALN.BFIT.LOCAL`).

## Requirements

- **REQ-ALN.BFIT.ICP.CORR-001** For every measured index in the resolved scope, the function shall return either a correspondence (foot point, unit normal, entity ID, unsigned distance) or `no_correspondence`, in measured-index order.
- **REQ-ALN.BFIT.ICP.CORR-002** When a maximum correspondence distance is set, points whose closest distance exceeds it shall be returned as `no_correspondence` with reason `beyond_max_distance`.
- **REQ-ALN.BFIT.ICP.CORR-003** When reference patches are given (local scope), the query shall consider only those triangles or faces.
- **REQ-ALN.BFIT.ICP.CORR-004** When two or more reference entities are equally close, the correspondence shall carry the lowest triangle index or face ID, as GEO.DIST guarantees, and this leaf's tests shall confirm it on the tie fixture.
- **REQ-ALN.BFIT.ICP.CORR-005** The normal shall be the triangle's face normal for interior feet, the GEO.DIST.SIGN pseudo-normal for edge and vertex feet, and the surface normal at the foot parameters for CAD faces, oriented outward.
- **REQ-ALN.BFIT.ICP.CORR-006** The result shall be byte-identical for 1, 2 or N threads.
- **REQ-ALN.BFIT.ICP.CORR-007** One correspondence pass of 1,000,000 points against a 1,000,000-triangle mesh shall take under 200 ms on reference hardware [unverified].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/aln/bfit/icp/cube_points.json` (points at known offsets from a unit-cube mesh), when `correspond` runs, then every foot and normal matches the analytic value within 1e-12 m and 1e-12.
- **AC-2** Given GEO's `shared_vertex_ties` fixture, then the lowest triangle index is reported for every tied point.
- **AC-3** Given `max_distance` 1 mm and a point 2 mm away, then it is `no_correspondence` with `beyond_max_distance`.
- **AC-4** Given 100,000 random points (seed 3) and 1, 2 and 8 threads, then the outputs are byte-identical.

## Design

- **Registry command(s):** none; R1 library function.
- **Function:** `correspond(points: &[P3], target: &dyn ProjectionTarget, opts: &CorrOptions, cancel: &CancelToken) -> Result<Correspondences, CorrError>`.
- **Data structures:** `CorrOptions { max_distance: Option<f64>, patches: Option<SortedIds> }`; `Correspondences { foot: Vec<P3>, normal: Vec<V3>, entity: Vec<u64>, dist: Vec<f64>, valid: BitVec, reason: Vec<CorrReason> }` (structure of arrays).
- **Algorithm:** GEO.DIST.BATCH projection with `max_distance` and `subset`; normals per the rule above.
- **Errors:** `frame_mismatch`, `cancelled`, `non_finite_query` (pass-through).
- **Determinism:** D0; GEO.DIST.BATCH is thread-count independent.
- **Performance budget:** REQ-007.
- **Crate:** `fid-align-solve` (kernel layer; provisional name).

## Test fixtures

`cube_points.json` (analytic, CC0); GEO's `shared_vertex_ties` (CC-BY-4.0, from GEO.DIST.MESH).

## Build prompt

> In crate `fid-align-solve` (pure Rust, f64, kernel layer, KNP-1), implement `correspond(points, target, opts, cancel) -> Result<Correspondences, CorrError>` over GEO's `ProjectionTarget` trait (`fid-geo-dist`). For each point in order return foot, outward unit normal (face normal inside a triangle, GEO's pseudo-normal on edges and vertices, surface normal at the foot for CAD faces), entity ID and distance, or `no_correspondence` with a reason (`no_hit`, `beyond_max_distance`). Honour `max_distance` and a sorted patch subset. Rely on GEO's lowest-ID tie-break and test it. Output must be byte-identical for any thread count. Tests: AC-1..AC-4. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, `fid-num`, fixed-order reductions, no std `HashMap`/`HashSet`, NaN is an error).

## Open questions

- GEO.DIST.MESH, GEO.DIST.BATCH and GEO.DIST.FACE are tagged P1b while this leaf is P1a. Either surface best-fit moves to P1b by decision or GEO.DIST's mesh path moves to P1a; raised for the GEO and ALN reviewers.
- **Build notes (fid-align-solve `icp::corr`, choices made while building):**
  - The reference is `Reference::Mesh(&SignTarget)` or `Reference::Cad(&CadReference)` (a GEO.DIST.FACE.BODY `FaceSetTarget` plus the GEO.DIST.FACE.INCID index of its body), not `&dyn ProjectionTarget`: that trait returns neither the foot's feature nor a normal, so the search calls `MeshTarget::closest` and `FaceSetTarget::closest` directly (the same GEO.DIST arithmetic and tie rules, without the sign, which ICP does not need). Placed instances (`InstanceTarget`) are not accepted yet.
  - Signature: `correspond(points: &PointSet, reference, opts: &CorrOptions, cancel: &AtomicBool) -> Result<CorrOutput, CorrError>`. The `PointSet` carries the frame for the `frame_mismatch` check; the kernel cancellation flag is an `AtomicBool`, as in GEO.DIST.BATCH; `CorrOptions` also holds `threads` (1..=256, never changes the output). `CorrOutput` holds the `Correspondences` (the METRIC-defined structure of arrays) plus per-outcome counts and notes (`normal_fallback`, `seam_combined`), so no outcome is dropped silently.
  - New outcome `normal_undefined` (an additive variant of METRIC's `CorrReason`, carrying the GEO.DIST.SIGN reason): where GEO.DIST gives no outward normal (a free or non-manifold edge, a vertex touching one, a misoriented component, a degenerate triangle, a cone apex), the point is not valid, but its foot, entity and distance are kept. On AC-2's fixture (open fans) the tied feet at boundary vertices are therefore reported with their lowest triangle index and `normal_undefined`.
  - CAD normal: the outward surface normal of the hit face at the foot parameters, also for feet on the face's boundary edge or corner (REQ-005 asks for the surface normal, not a pseudo-normal). On a NURBS seam, where INCID routes the foot to its edge rule, the normal is the angle-weighted sum of both sides, normalised and noted. Patch IDs for CAD are face indexes of the body (`FaceIx::index`).
  - Extra refusals, all fail closed: `invalid_threads`, `invalid_max_distance`, `empty_patches`, `invalid_patches` (position), `empty_reference`, `no_points`, `query_out_of_range`, `allocation_failed`, `worker_failed`, `cad_reference`; a search or normal failure fails the whole call with the lowest index.
  - Ties: GEO.DIST returns only the winner, so this leaf cannot report that a tie happened. A per-hit tie count from GEO.DIST.MESH and GEO.DIST.FACE.BODY would close that (cross-node follow-up).
  - REQ-007 (200 ms for 1e6 points on a 1e6-triangle mesh) is not measured.

## Children

_Leaf: none._
