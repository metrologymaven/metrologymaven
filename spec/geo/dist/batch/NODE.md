---
id: GEO.DIST.BATCH
title: Deterministic batch projection and options
level: 3
parent: GEO.DIST
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-GEO.DIST-004, REQ-GEO.DIST-005, REQ-GEO.DIST-006, REQ-GEO.DIST-007]
covers: []
depends_on: [GEO.DIST.MESH, GEO.DIST.SIGN, GEO.FRAME.APPLY]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GEO.DIST.BATCH: Deterministic batch projection and options

## Purpose

Projects millions of points at once onto a target, in parallel, with results that do not depend on thread count or batch partitioning, and applies the projection options (maximum distance, subset restriction, projection along a direction) and the frame check uniformly for every target kind.

## Scope

**In:**
- `ProjectionTarget` trait implemented for meshes here and for CAD faces and bodies by GEO.DIST.FACE
- Fixed-size blocks (4 096 points) processed in parallel, results written by input index
- Options: `max_distance`, target subset, `direction` (project along ±v: nearest intersection of the line through p with the target)
- Frame check through `require_same_frame` before any work
- Summary statistics (counts of hits, no-hits, undefined signs; min, max and mean signed distance with a fixed-order sum)

**Out:**
- Per-point geometry (owned by `GEO.DIST.MESH`, `GEO.DIST.SIGN`, `GEO.DIST.FACE`)
- The registry command (owned by `GEO.DIST.CMD`)
- Deviation maps and statistics of record (owned by `SCAN`)

## Requirements

- **REQ-GEO.DIST.BATCH-001** Batch projection with 1, 2 or N threads, and with any block size that is a multiple of 64, shall produce byte-identical results.
- **REQ-GEO.DIST.BATCH-002** If the query points and the target are in different frames, then the batch shall fail with the GEO.FRAME `FrameMismatch` error before projecting any point.
- **REQ-GEO.DIST.BATCH-003** When a direction v is given, the batch shall return for each point the nearest intersection of the line p + s·v (s of either sign) with the target, or `NoHit` if none lies within `max_distance`.
- **REQ-GEO.DIST.BATCH-004** Summary statistics shall be computed with fixed-order reductions over input order, so they are identical regardless of thread count.
- **REQ-GEO.DIST.BATCH-005** Batch projection of 10⁷ points onto a 10⁶-triangle mesh shall complete in under 10 s on reference hardware [unverified].
- **REQ-GEO.DIST.BATCH-006** The batch shall check a cancellation token between blocks and, if cancelled, return `Cancelled` with no partial results.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/dist/scan_1e6_on_mesh` (10⁶ seeded points near `mesh_sphere_1e5`), when projected with 1, 3 and 16 threads and block sizes 64 and 4 096, then all result buffers are byte-identical.
- **AC-2** (error path) Given points in frame A and the mesh in frame B, when the batch runs, then it fails with `FrameMismatch` and no point is processed.
- **AC-3** Given fixture `fixtures/geo/dist/directional_cases.json` (points with direction vectors and exact rational intersection references), when projected along v, then hits match within 1e-12 m and misses are `NoHit`.
- **AC-4** Given AC-1 inputs, when summary statistics are computed with different thread counts, then they are bit-identical.
- **AC-5** Given a cancellation after the first block, when the batch runs, then it returns `Cancelled` and no results.
- **AC-6** Given a seeded 10⁷-point set and a 10⁶-triangle mesh on the reference machine, when timed by the bench target, then the run completes in under 10 s (advisory until the reference machine exists).

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `trait ProjectionTarget { fn frame(&self) -> FrameRef; fn project(&self, p, opts) -> Projection; fn project_along(&self, p, v, opts) -> Projection; }`; `ProjectionOptions { max_distance, subset, direction }`; `Projection { foot, signed_distance, sign_defined, entity_id, params: Option<(f64, f64)> } | NoHit`; `BatchSummary`.
- **Algorithm:** split input into fixed blocks; a thread pool processes blocks in any order and writes into pre-allocated slots by index; summaries reduce in input order after all blocks finish. Directional mesh queries use `PrimitiveBvh::ray` with a watertight ray–triangle test (Woop, Benthin and Wald, JCGT 2013) [unverified reference details].
- **Errors:** `FrameMismatch`, `Cancelled`, `NonFiniteQuery`.
- **Determinism:** D0 (per-point results are D0; scheduling cannot affect slot contents or reduction order).
- **Performance budget:** as REQ-GEO.DIST.BATCH-005.
- **Crate:** `fid-geo-dist` (kernel graph, KNP-1 applies; the cancellation token is a plain atomic flag passed in by the caller).

## Test fixtures

- `scan_1e6_on_mesh` (seed 51) and `directional_cases.json` from `tools/fixtures/geo_dist.py`. CC-BY-4.0.

## Build prompt

> In crate `fid-geo-dist` (pure Rust, no I/O, KNP-1), add module `batch`: trait `ProjectionTarget` (frame, project, project_along), its implementation for meshes (using modules `mesh` and `sign` and `PrimitiveBvh`), `ProjectionOptions { max_distance, subset, direction }`, and `project_batch(points, target, opts, threads, cancel) -> Result<Vec<Projection>, DistError>` plus `summary(&[Projection]) -> BatchSummary`. Check frames first with `require_same_frame` (`FrameMismatch`). Process fixed blocks (default 4 096, any multiple of 64) in parallel writing results by index; check the cancel flag between blocks (`Cancelled`, no partial results); reduce summaries in input order with `fid-num` fixed-order sums. Directional projection intersects the line p + s·v (either sign) using a watertight ray–triangle test and returns the nearest hit within `max_distance`. Acceptance: AC-1 byte-identical results for thread counts 1/3/16 and block sizes 64/4 096; AC-2 frame mismatch; AC-3 directional cases within 1e-12 m; AC-4 identical summaries; AC-5 cancellation; AC-6 bench under 10 s (advisory). Constraints: CONSTITUTION §8 (no panic, no `unsafe`), KNP-1. Files: `crates/fid-geo-dist/src/batch.rs`, `tests/batch.rs`, `benches/project.rs`.

## Open questions

- Reference hardware for REQ-GEO.DIST.BATCH-005 depends on D-014 and PLAT.

## Children

_Leaf: none._
