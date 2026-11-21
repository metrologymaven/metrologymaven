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
built_in: [fid-geo-dist]
satisfies: [REQ-GEO.DIST-004, REQ-GEO.DIST-005, REQ-GEO.DIST-006, REQ-GEO.DIST-007]
covers: []
depends_on: [GEO.DIST.MESH, GEO.DIST.SIGN, GEO.FRAME.APPLY]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-20
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

### Pinned at build (fid-geo-dist module `batch`)

These pins were made when the leaf was built. Where one differs from the text above, the pin wins, and the reason is given.

- **API.** `project_batch(points: &PointSet, target: &T, opts: &ProjectionOptions, config: &BatchConfig { threads, block }, cancel: &AtomicBool) -> Result<Vec<Projection>, BatchError>`, `summary(&[Projection]) -> Result<BatchSummary, BatchError>` and `encode(&[Projection])`, which writes the pinned 67-byte record per result: tag, entity `u64`, foot, distance, signed distance, sign code, params. Threads go from 1 to 256, and 1 runs on the calling thread. The block is a positive multiple of 64, at most 2^20.
- **Trait.** `ProjectionTarget: Sync { frame(); project(p, opts); project_along(p, v, opts) }` returns `Result<Projection, ProjectError>` (`ProjectError` is `Sign(SignError)` or `InvalidDirection`). The mesh implementation is `SignTarget` itself, because the batch needs the pseudo-normals for the sign. The subset option is a `TriangleSubset` (strictly ascending IDs below a count, validated once), so GEO.DIST.FACE can reuse it for face IDs.
- **Projection** is `Hit(ProjectionHit { foot, distance, signed_distance, sign: HitSign, entity_id, params })` or `NoHit`. `HitSign` is `Mesh(SignStatus)` for a closest-point projection, and `Along` for a directional one. A directional foot is not a closest point, so the GEO.DIST.SIGN rule does not apply. Its signed distance is `t |v|` (positive along `+v`), and its sign counts as defined. The Design's `sign_defined` field is `ProjectionHit::sign_defined()`, and `distance` is added. `params` is `None` for meshes.
- **Order of checks.** Frames first (`require_same_frame_ref`, `FrameMismatch`, before even the configuration is looked at), then threads and block size, then `max_distance` and `direction`, then every point (`NonFiniteQuery`, `QueryOutOfRange` for coordinates beyond 2^250), then the allocation (`try_reserve`). `PointSet` already refuses non-finite coordinates, so `NonFiniteQuery` is defensive. Added errors: `InvalidThreads`, `InvalidBlockSize`, `InvalidMaxDistance`, `InvalidDirection`, `QueryOutOfRange`, `AllocationFailed`, `Point { index, error }` and `WorkerFailed`.
- **Scheduling.** Blocks are cut in input order, and workers take them from a shared queue. `std::thread::scope` is used, with `Builder::spawn_scoped`, so a thread that cannot start is not a panic: the other workers take its blocks. Each result is written into its slot by index. When points fail, the lowest failing index is returned, whatever the schedule: blocks after a known failure may be skipped, but every earlier block completes. Cancellation is read before each block. If the flag is set at any time before the batch returns, the result is `Cancelled`, with no partial results.
- **Memory.** It is bounded by the output: one `Projection` per point, plus one queue entry per block, plus per-point candidate lists of the hierarchy. All are allocated fallibly where the size is known in advance.
- **Summary.** Counts are points, hits, no-hits and `sign_undefined` (hits whose sign is not defined). Min, max and mean are over the hits whose sign is defined. Min and max use `fid_num::f::total_min`/`total_max` in input order (so `-0.0 < +0.0`), and the mean is `fid_num::red::mean`. It is computed on one thread after the batch.
- **Directional projection on a mesh.** `v` is scaled up by exact powers of two until `max |v_k| >= 4`, so the line is unchanged and every intersection parameter is at most 2^249. Candidates come from `PrimitiveBvh::ray(p, v, -2^250, 2^250)` over the whole line, so the candidate set does not depend on `max_distance`. Each candidate gets the watertight shear test of Woop, Benthin and Wald (JCGT 2013; our own code). The exact formulas are in the module docs, and it is f64 throughout, with no higher-precision fallback. Edges count as inside. A coplanar line (`det == 0`) misses. The smallest `|t|` wins, ties going to the lowest triangle index. The distance is `|t| * |v|`, and the result is `NoHit` beyond `max_distance`. `SignTarget::project_along_brute` gives the same result bit for bit without the hierarchy.
- **Fixtures.** `tools/fixtures/geo_dist_batch.py` (stdlib only) writes `fixtures/geo/dist/scan_1e6_on_mesh/expected.txt` and `fixtures/geo/dist/directional_cases.json`. The scan file holds the digest of the 10^6-point stream (SplitMix64 seed 51, points near the GEO.INDEX sphere `mesh_sphere_1e5`) and the closest point of every 10,000th point, from the GEO.DIST.MESH reference model in `tools/fixtures/geo_dist.py`. The directional file holds 135 cases on a cube and a tetrahedron, intersected in exact rational arithmetic: 15 designed cases (shared edges and vertices, negative `s`, tiny and huge `v`, `max_distance` at and below the hit, subsets) and 120 random, well-conditioned ones. CI runs the script with `--check`. Plain `cargo test` runs AC-1 and AC-4 on the first 50,000 points with thread counts 1, 3, 7 and 16 and block sizes 64, 192 and 4,096. A Linux CI step runs the full 10^6 points in release (`FID_BATCH_POINTS=1000000`). The AC-6 bench is `benches/project.rs` (advisory, prints only).

## Test fixtures

- `scan_1e6_on_mesh` (seed 51) and `directional_cases.json` from `tools/fixtures/geo_dist.py`. CC-BY-4.0.

## Build prompt

> In crate `fid-geo-dist` (pure Rust, no I/O, KNP-1), add module `batch`: trait `ProjectionTarget` (frame, project, project_along), its implementation for meshes (using modules `mesh` and `sign` and `PrimitiveBvh`), `ProjectionOptions { max_distance, subset, direction }`, and `project_batch(points, target, opts, threads, cancel) -> Result<Vec<Projection>, DistError>` plus `summary(&[Projection]) -> BatchSummary`. Check frames first with `require_same_frame` (`FrameMismatch`). Process fixed blocks (default 4 096, any multiple of 64) in parallel writing results by index; check the cancel flag between blocks (`Cancelled`, no partial results); reduce summaries in input order with `fid-num` fixed-order sums. Directional projection intersects the line p + s·v (either sign) using a watertight ray–triangle test and returns the nearest hit within `max_distance`. Acceptance: AC-1 byte-identical results for thread counts 1/3/16 and block sizes 64/4 096; AC-2 frame mismatch; AC-3 directional cases within 1e-12 m; AC-4 identical summaries; AC-5 cancellation; AC-6 bench under 10 s (advisory). Constraints: CONSTITUTION §8 (no panic, no `unsafe`), KNP-1. Files: `crates/fid-geo-dist/src/batch.rs`, `tests/batch.rs`, `benches/project.rs`.

## Open questions

- Reference hardware for REQ-GEO.DIST.BATCH-005 depends on D-014 and PLAT.

## Children

_Leaf: none._
