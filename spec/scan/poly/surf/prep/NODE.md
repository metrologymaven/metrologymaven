---
id: SCAN.POLY.SURF.PREP
title: Resolution resampling and bounded smoothing before meshing
level: 4
parent: SCAN.POLY.SURF
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
built_in: [fid-scan-poly]
built_pr: 653
satisfies: [REQ-SCAN.POLY.SURF-002, REQ-SCAN.POLY.SURF-003, REQ-SCAN.POLY.SURF-005]
covers: []
depends_on: [GEO.INDEX.KDTREE, GEO.TYPES.POINTS, KRN.NUM.LA.EIG]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# SCAN.POLY.SURF.PREP: Resolution resampling and bounded smoothing before meshing

## Purpose

Prepares the vertex set the reconstructor triangulates: one representative input point per cell of a grid at the target resolution, and, when smoothing is on, a bounded move of each representative onto its local fitted plane, skipped near sharp edges when sharp-edge preservation is on. Because vertices start as measured points and moves are bounded, the fidelity bound of the parent holds by construction.

## Scope

**In:**
- Grid representative: per occupied cell (edge = target resolution h), the valid-normal input point nearest the cell's point centroid (ties lowest index)
- Optional smoothing: projection onto the least-squares plane of neighbours within radius 2h, clamped to a displacement of h/2
- Sharp-edge test: skip smoothing where the neighbourhood normal spread exceeds a threshold angle (default 30°)
- Output: vertex positions, normals and the source input index per vertex

**Out** (and which node owns it instead):
- Triangulation (owned by `SCAN.POLY.SURF.FAN` and `SCAN.POLY.SURF.STITCH`)
- Chunked execution (owned by `SCAN.POLY.SURF.OOC`)

## Requirements

- **REQ-SCAN.POLY.SURF.PREP-001** For each occupied grid cell of edge h, the function shall select the input point with a valid normal nearest the cell's centroid of valid points, ties by lowest index, and record its input index.
- **REQ-SCAN.POLY.SURF.PREP-002** When smoothing is on, each representative shall move to its projection on the least-squares plane of its valid neighbours within 2h, clamped so the displacement is at most h/2, except where sharp-edge preservation is on and the neighbourhood normal spread exceeds the threshold.
- **REQ-SCAN.POLY.SURF.PREP-003** Every output vertex shall lie within h/2 of its source input point, and the result shall be D0 and independent of thread count.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/poly/plane_dense` (0.1 mm spacing) and h = 1 mm, one representative per cell is produced and each is an unmodified input point when smoothing is off.
- **AC-2** Given the noisy variant (σ = 20 µm) with smoothing on, the RMS distance of vertices to the true plane drops by at least half and no vertex moved more than h/2.
- **AC-3** Given fixture `fixtures/scan/poly/cube_edges` with sharp-edge preservation on, vertices within h of an edge are not moved.
- **AC-4** Given 1 and 16 threads, outputs are identical.

## Design

- **API** (`fid-scan-poly::prep`): `fn prepare(cloud: &PointCloudView, valid: &Bitmask, params: &PrepParams) -> Result<VertexSet, PolyError>`.
- **Algorithm:** cell key = floor((x − origin)/h) with origin = cloud bounding-box minimum; cells sorted by Morton code; centroid by fixed-order sums; plane fit by `fid-num` symmetric eigensolver; clamp by scaling the displacement vector.
- **Errors:** `scan.poly.invalid_parameter`.
- **Determinism:** D0.
- **Crate:** `fid-scan-poly` (kernel tier, KNP-1).

## Test fixtures

`plane_dense`, `cube_edges` (seeded synthetic, CC0).

## Build prompt

> In a new Rust crate `fid-scan-poly`, module `prep`, implement `prepare(cloud, valid, params) -> VertexSet`: grid of edge h from the bounding-box minimum, per occupied cell (Morton order) the valid-normal point nearest the cell centroid (ties lowest index); optional smoothing onto the least-squares plane of valid neighbours within 2h (`fid-num` eigensolver), displacement clamped to h/2, skipped where sharp-edge preservation is on and the neighbour normal spread exceeds the threshold. Record each vertex's source index. Constraints: KNP-1 (no FMA, fixed-order sums, no std HashMap/HashSet), §8, D0. Tests: AC-1..AC-4.

## Open questions

- **Built in `fid-scan-poly` (2026-11-26); choices made at build time.** The API is `prepare(cloud: &PointCloud, valid: &[bool], params: &PrepParams) -> Result<VertexSet, PolyError>`: GEO.TYPES has no `PointCloudView` or `Bitmask` type, and SCAN.CLOUD.NORM (the producer of the valid mask) is not built yet, so the mask is one `bool` per point. Pinned in the crate's `prep` module docs: the bounding box is over all points; neighbours are the valid points within 2h of the representative (itself included), via the GEO.INDEX.KDTREE radius query; "normal spread exceeds the threshold" means some neighbour normal has a dot product with the representative's normal below cos(threshold), default 30 degrees [unverified]; fewer than 3 neighbours or a plane rank below 2 keeps the point; the clamp aims at (h/2)(1 - 2^-40) and a rounded displacement above h/2 keeps the point. Besides `scan.poly.invalid_parameter` the crate reports `scan.poly.out_of_range`, `index`, `numeric`, `allocation`, `worker_failed` and `inconsistent`. Threads: contiguous blocks of representatives on scoped threads, joined in order (as in `fid-geo-dist`); wasm32 runs them on the calling thread.

## Children

_Leaf: none._
