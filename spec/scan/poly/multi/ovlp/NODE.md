---
id: SCAN.POLY.MULTI.OVLP
title: Per-point overlap detection between scans
level: 4
parent: SCAN.POLY.MULTI
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
built_in: [fid-scan-poly]
satisfies: [REQ-SCAN.POLY.MULTI-002, REQ-SCAN.POLY.MULTI-004]
covers: []
depends_on: [GEO.INDEX.KDTREE, GEO.OOC.QUERY, GEO.TYPES.POINTS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# SCAN.POLY.MULTI.OVLP: Per-point overlap detection between scans

## Purpose

Marks, for every point of every registered scan, which other scans have measured the same surface nearby: another scan overlaps a point when it has a point within the overlap distance d whose normal faces the same way. The result is the input to overlap reduction.

## Scope

**In:**
- One k-d tree per scan (in memory or chunked)
- Per point: sorted list (bitset for up to 64 scans, sorted vector above) of other scans with a point within d and normal angle below 60°
- Summary: overlapping point count per scan pair

**Out** (and which node owns it instead):
- Choosing which points to keep (owned by `SCAN.POLY.MULTI.REDUCE`)
- Registration (owned by `SCAN.CLOUD.REG`)

## Requirements

- **REQ-SCAN.POLY.MULTI.OVLP-001** For each point p of scan i, the function shall record scan j ≠ i as overlapping exactly when scan j has a point within Euclidean distance d of p whose normal makes an angle below 60° with p's normal.
- **REQ-SCAN.POLY.MULTI.OVLP-002** Points without valid normals shall be marked overlapping with scan j when any point of j lies within d.
- **REQ-SCAN.POLY.MULTI.OVLP-003** The function shall be D0 and independent of thread count and in-memory versus chunked storage.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/poly/two_sheets_overlap` (two registered scans of a plate overlapping on a 20 mm strip), points inside the strip are marked overlapping and points outside are not, with d = 0.3 mm.
- **AC-2** Given the two sides of a 1 mm-thick wall (normals opposed), points are not marked overlapping across the wall.
- **AC-3** Given chunked and in-memory inputs, results are identical.

## Design

- **API** (`fid-scan-poly::multi::overlap`): `fn detect_overlap(scans: &[ScanView], d: f64) -> Result<OverlapMap, PolyError>`.
- **Algorithm:** for each scan j, k-d tree; for each point of i, radius query on each other scan whose bounding box inflated by d contains it; stop at the first compatible neighbour.
- **Errors:** `scan.poly.invalid_parameter`.
- **Determinism:** D0.
- **Crate:** `fid-scan-poly`.

## Test fixtures

`two_sheets_overlap`, `thin_wall_two_scans` (seeded synthetic, CC0).

## Build prompt

> In the Rust crate `fid-scan-poly`, module `multi::overlap`, implement `detect_overlap(scans, d)`: per point of scan i, the sorted set of other scans j with a point within d whose normal is within 60° of the point's normal (any point within d if the point has no valid normal), using per-scan k-d trees (`fid-geo-index`, or GEO.OOC `ChunkedIndex` for chunked clouds) with a bounding-box prefilter. Return per-point sets and per-pair counts. Constraints: KNP-1, §8, D0. Tests: AC-1..AC-3.

## Open questions

- **Built (fid-scan-poly `multi::overlap`), choices made at the build:** the API is `detect_overlap(scans: &[&dyn ScanView], d) -> Result<OverlapMap, PolyError>`. `ScanView` is a kernel trait (frame, point count, point position and optional normal, GEO.INDEX-contract radius query) and `MemoryScan` is its in-memory implementation over a GEO.TYPES `PointCloud` and SCAN.CLOUD.NORM validity flags. This kernel crate cannot depend on GEO.OOC (model layer), so the chunked view used for AC-3 is a test adapter over `ChunkedIndex` and the chunk cache in `crates/fid-geo-ooc/tests/scan_overlap.rs`. A production chunked `ScanView` belongs to `SCAN.POLY.MULTI.CMD` (or GEO.OOC). GEO.OOC has no point-attribute lookup by original index, and the test adapter keeps an in-memory index-to-chunk map, so bounded memory for chunked attribute access is open for the GEO.OOC owner.
- "Angle below 60 degrees" is pinned as the normal dot product strictly above 0.5 (`OVERLAP_NORMAL_COS`), so exactly 60 degrees does not count. SCAN.POLY.SURF.FAN uses `>= 0.5` ("within 60 degrees"). A neighbour without a valid normal does not count for a point that has one (REQ-001 needs the neighbour's normal); a point without one counts any neighbour within d (REQ-002). "Within d" is the GEO.INDEX pinned `dist2 <= d*d`, so a neighbour at exactly d counts.
- Fail-closed additions: d must be finite in [2^-100, 2^100] m; at least two scans; all scans in one frame (`scans.frame`); every position and present normal is validated before any query; a view hit beyond d or out of range is refused as `scan.poly.inconsistent`; a view's own storage error is passed through as `scan.poly.view` with the view's code (a new additive `PolyError::View` variant). Every point carries a status (`overlapping`, `normals_incompatible`, `nothing_within_d`) and the rule that applied, so an empty set always has a reason, and an empty scan is reported with 0 points.
- Pair counts are directed (points of scan i that scan j overlaps; overlap is not symmetric) and only non-zero pairs are listed (`pair_count` gives 0 for the rest). The detection runs on the calling thread, so REQ-003's thread-count independence holds trivially; a threaded variant is a possible follow-up.
- AC-2's d is not stated; `thin_wall_two_scans` uses d = 1.5 mm, larger than the 1 mm wall, so only the normals keep the faces apart (at d = 0.3 mm the faces would be too far apart to test anything).

## Children

_Leaf: none._
