---
id: SCAN.CLOUD.REG.GRAPH
title: Scan overlap matrix and pair graph
level: 4
parent: SCAN.CLOUD.REG
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
built_in: [fid-scan-cloud]
built_pr: 679
satisfies: [REQ-SCAN.CLOUD.REG-003, REQ-SCAN.CLOUD.REG-006, REQ-SCAN.CLOUD.REG-008]
covers: []
depends_on: [SCAN.CLOUD.REG.CTGT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# SCAN.CLOUD.REG.GRAPH: Scan overlap matrix and pair graph

## Purpose

Decides which scan pairs take part in global registration: it computes the overlap matrix of N scans at their current poses, keeps the pairs above a minimum overlap, and checks that the resulting graph is connected. A disconnected graph means some scans cannot be tied to the rest, which must be an explicit error rather than a silently unconstrained pose.

## Scope

**In:**
- Overlap matrix f(i, j) for all ordered pairs, using `overlap_fraction`
- Pair graph: undirected edge (i, j) when min(f(i, j), f(j, i)) ≥ the minimum overlap
- Connected components and the `disconnected` error

**Out** (and which node owns it instead):
- The overlap definition and sampling (owned by `SCAN.CLOUD.REG.CTGT`)
- The joint adjustment over the graph (owned by `SCAN.CLOUD.REG.GLOBAL`)

## Requirements

- **REQ-SCAN.CLOUD.REG.GRAPH-001** The function shall compute f(i, j) for every ordered pair i ≠ j of the N input scans with `overlap_fraction` at the stated overlap distance and sample cap.
- **REQ-SCAN.CLOUD.REG.GRAPH-002** The pair graph shall contain edge (i, j), i < j, exactly when min(f(i, j), f(j, i)) is at least the stated minimum overlap (default 0.1), and edges shall be listed in ascending (i, j) order.
- **REQ-SCAN.CLOUD.REG.GRAPH-003** If the pair graph has more than one connected component, then the function shall return `scan.cloud.reg_disconnected` listing each component as a sorted list of scan indexes, components ordered by their smallest index.
- **REQ-SCAN.CLOUD.REG.GRAPH-004** Pairs whose axis-aligned bounding boxes, inflated by the overlap distance, do not intersect shall get f = 0 without a neighbour query.
- **REQ-SCAN.CLOUD.REG.GRAPH-005** The function shall be determinism class D0.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/reg/three_scans_known` at true poses, the graph has edges (0,1), (1,2) and (0,2) with the fixture's design overlaps ± 0.01.
- **AC-2** Given fixture `fixtures/scan/reg/four_scans_two_islands`, the function returns `scan.cloud.reg_disconnected` with components [[0,1],[2,3]].
- **AC-3** Given minimum overlap 0.5 on the three-scan fixture, only pairs above 0.5 remain and the error is raised if that disconnects the graph.
- **AC-4** Given 1 and 16 threads, the matrix is bit-identical.

## Design

- **API** (`fid-scan-cloud::reg::graph`): `fn pair_graph(scans: &[ScanInput], poses: &[Rigid], d: f64, min_overlap: f64, max_samples: u64) -> Result<PairGraph, RegError>`; `PairGraph { overlap: Vec<f64> /* row-major N×N, diagonal 1 */, edges: Vec<(u32, u32)> }`.
- **Algorithm:** bounding-box prefilter; overlap per ordered pair in ascending (i, j); union-find over edges with lowest-index roots.
- **Errors:** `scan.cloud.reg_disconnected { components }`, `scan.cloud.invalid_parameter` (N < 2, min_overlap outside (0, 1]).
- **Determinism:** D0.
- **Performance budget:** 50 scans × 200 000 samples under 60 s [unverified].
- **Crate:** `fid-scan-cloud`.

## Test fixtures

`three_scans_known` (shared), `four_scans_two_islands` (seeded synthetic), CC0.

## Build prompt

> In the Rust crate `fid-scan-cloud`, module `reg::graph`, implement `pair_graph(scans, poses, d, min_overlap, max_samples)`. For each ordered pair i ≠ j (ascending), set f(i, j) = 0 if the bounding boxes inflated by d do not meet, else `overlap_fraction` (module `reg::target`) of scan i at pose i against an `OrientedCloudTarget`-backed nearest-point query on scan j at pose j. Keep edge (i, j), i < j, when min(f(i,j), f(j,i)) ≥ min_overlap (default 0.1). Union-find for components; more than one component fails `scan.cloud.reg_disconnected` with sorted components ordered by smallest index. Pure, D0, thread-count independent.
>
> Constraints: CONSTITUTION §8, KNP-1 (no FMA, no std HashMap/HashSet, total-order ties). Tests: AC-1..AC-4.

## Open questions

- Built in `fid-scan-cloud` `reg::graph` (build notes, 2026-11-27). `ScanInput` is not defined elsewhere, so it is `{ cloud: &PointCloud, valid: &Bitmask }` (the scan in its own scanner coordinates and the valid-normal mask of its target points). Scan j's target is `OrientedCloudTarget::new(transform_element(scan j, pose j), valid j)` (GEO.FRAME.APPLY), and f(i, j) is `overlap_fraction(scan i, pose i, that target)`, so both scans sit at their poses in the one frame of scan 0; a scan in another frame is refused (`geo.frame.mismatch`), never converted.
- The rounding floor of `d` is checked once over all scans (`64 * 2^-53 * M`, M over every sampled point, image, translation and target coordinate), so a pair skipped by the REQ-004 prefilter cannot hide a `d` below rounding resolution. The prefilter test (gap `g > 0` and `sqrt(g * g) > d` on some axis) is proven value-preserving (module docs) and checked against the full computation in the tests.
- N is capped at `MAX_SCANS` = 1024 (`scan.cloud.invalid_parameter`), since the matrix has N² cells; the spec budget is 50 scans.
- `scan.cloud.reg_disconnected` (`RegError::Disconnected`) carries the components and the whole `PairGraph`, so GLOBAL can report the overlap it found. Errors of one scan or pair are wrapped (`RegError::Scan`, `RegError::Pair`) with their indexes and keep the inner code.
- The shared fixture `three_scans_known` is noise-free (three 17 x 11 lattice patches at 4 mm pitch of one polynomial freeform part, exact integer-quaternion poses); PAIR's REQ-006 noise of 10 µm is for PAIR to add with its own seed. Generator: `fixtures/scan/reg/make_graph_fixtures.py`.
- The performance budget (50 scans × 200 000 samples under 60 s) stays [unverified]: not measured.

## Children

_Leaf: none._
