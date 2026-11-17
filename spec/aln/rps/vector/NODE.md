---
id: ALN.RPS.VECTOR
title: Sheet-metal iterative alignment on vector points
level: 3
parent: ALN.RPS
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-ALN.RPS-003, REQ-ALN.RPS-006, REQ-ALN.RPS-007]
covers: [PCD.ALN.03]
depends_on: [ALN.MODEL.ELEM, ALN.QA.REPORT, GEO.DIST, KRN.SOLV]
cli: [fid align run-rps-vector]
mcp: [align_run_rps_vector]
journal: [align.run_rps_vector@1]
updated: 2026-11-17
---

# ALN.RPS.VECTOR: Sheet-metal iterative alignment on vector points

## Purpose

The sheet-metal iterative alignment on vector points: each point has a nominal position and a nominal vector, and its deviation is measured along that vector. On every iteration the system re-evaluates each point's deviation along its nominal vector under the current pose (for CMM hits, by projecting the measured hit onto the nominal vector line; for scan data, by intersecting the nominal vector line with the measured surface) and re-solves the locked-direction problem until the threshold is met.

## Scope

**In:**
- Vector-point inputs: nominal position, nominal unit vector, lock flag per point, and a measured source (discrete hit or measured surface)
- Per-iteration re-evaluation of deviations along nominal vectors for both sources
- The outer loop with threshold (shared default with REQ-ALN.RPS-003) and iteration limit
- `align.run_rps_vector@1`

**Out** (and which node owns it instead):
- Frame-axis RPS locks (`ALN.RPS.LOCK`); the per-iteration solve here uses KRN.SOLV directly (see Open questions)
- Re-measurement on a machine (`CMM`)
- Sheet-metal pack checks such as trim, hem and hole position (`IND.SHEET`)
- Ray-mesh intersection primitives (`GEO.DIST`)

## Requirements

- **REQ-ALN.RPS.VECTOR-001** On each iteration, the system shall re-evaluate every vector point's deviation along its nominal vector under the current pose before re-solving.
- **REQ-ALN.RPS.VECTOR-002** For a measured-surface source, the deviation shall be the signed distance along the nominal vector line to its first intersection with the measured surface within a search length (default 5 mm [unverified]), and a point with no intersection shall be excluded and reported `no_intersection`.
- **REQ-ALN.RPS.VECTOR-003** The loop shall stop when the largest change in locked-direction deviation is below the threshold (default 1e-6 m [unverified]) or at the iteration limit (default 50), recording count, reason and per-iteration change; a stop by limit shall give status `not_converged`.
- **REQ-ALN.RPS.VECTOR-004** If, after exclusions, the remaining locks do not total six independent DoF, then the command shall fail with `rps_dof_mismatch` and journal nothing.
- **REQ-ALN.RPS.VECTOR-005** Given noise-free synthetic vector points on a known panel and a known transform, the converged locked-direction deviations shall be at most 1e-9 m [unverified].
- **REQ-ALN.RPS.VECTOR-006** The iteration shall run in the CPU kernel under KNP-1 and declare class D0 (fixed schedule) or D3.

## Interfaces

- **Offers:** `align.run_rps_vector@1` (W2; params `points` with nominal position, vector, lock flag and source; `threshold`, `max_iterations`, `search_length`, `parent`).
- **Consumes:** KRN.SOLV NLLS solve; GEO ray-surface intersection; `ALN.MODEL.ELEM`; `ALN.QA.REPORT`.

## Decomposition hints

HITS (discrete-hit deviation along vectors), SURF (scan-surface intersection along vectors), LOOP (outer iteration and command). HITS and LOOP are likely S leaves; SURF depends on GEO.DIST's ray queries.

## Open questions

- Hints HITS and LOOP are merged into LOOP (the hit provider is a few lines); SURF is its own P1b leaf because it needs GEO.DIST.BATCH's directional projection (P1b). LOOP calls the kernel NLLS driver directly because ALN.RPS.LOCK only locks shared frame axes, so `ALN.RPS.LOCK` is removed from depends_on. `ALN.RPS.ITER` reuses LOOP.
- Whether unlocked vector points should contribute to the fit with low weight (some incumbents' "best-fit with RPS" variants) [unverified]; currently they are reported only.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| ALN.RPS.VECTOR.LOOP | Vector-point iterative locked-direction solve (leaf) | 001 003 004 005 006 | none |
| ALN.RPS.VECTOR.SURF | Measured-surface deviation provider for vector points (leaf, P1b) | 001 002 | none |
| ALN.RPS.VECTOR.CMD | Vector-point alignment command and alignment wrapping (leaf) | 003 004 | PCD.ALN.03 |
