---
id: SCAN.POLY.SURF.OOC
title: Chunked reconstruction identical to in-memory
level: 4
parent: SCAN.POLY.SURF
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.POLY.SURF-003, REQ-SCAN.POLY.SURF-007]
covers: []
depends_on: [SCAN.POLY.SURF.PREP, SCAN.POLY.SURF.FAN, SCAN.POLY.SURF.STITCH, GEO.OOC.STREAM, GEO.OOC.QUERY]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# SCAN.POLY.SURF.OOC: Chunked reconstruction identical to in-memory

## Purpose

Runs the reconstruction on clouds too large for memory by processing spatial tiles with a halo, and guarantees the same mesh as the in-memory run. This works because PREP cells and FAN fans are local: a tile owns the cells and fans whose centre lies inside it, and a halo of at least 2h + g supplies every neighbour those need. It also provides `reconstruct`, the single kernel entry that dispatches between in-memory and chunked paths and that `SCAN.POLY.MULTI` calls.

## Scope

**In:**
- Tiles aligned to the PREP grid (tile edge a multiple of h), processed in Morton order; halo width 2h + g
- Ownership: a cell belongs to the tile containing its index; a fan to the tile owning its centre vertex
- Global vertex numbering by (cell Morton code) so indexes match the in-memory run
- Votes gathered from all fans and stitched once (sorted external merge when above the memory budget)
- `reconstruct(cloud, params, budget) -> Mesh`

**Out** (and which node owns it instead):
- Chunk storage and queries (owned by `GEO.OOC`)

## Requirements

- **REQ-SCAN.POLY.SURF.OOC-001** For any tile size that is a multiple of h and any thread count, the chunked mesh digest shall equal the in-memory mesh digest for the same cloud and parameters.
- **REQ-SCAN.POLY.SURF.OOC-002** Vertex indexes shall be assigned in ascending cell Morton order in both paths.
- **REQ-SCAN.POLY.SURF.OOC-003** `reconstruct` shall choose the in-memory path when the estimated peak memory is within the budget and the chunked path otherwise, with identical results either way.
- **REQ-SCAN.POLY.SURF.OOC-004** Reconstruction of a 10⁸-point cloud shall complete within 1 800 s and 32 GiB peak resident memory on reference hardware [unverified].

## Acceptance criteria

- **AC-1** Given `sphere_points` stored as chunks with tile edges 8h and 32h, mesh digests equal the in-memory digest.
- **AC-2** Given a forced memory budget below the in-memory estimate, the chunked path runs and the digest still matches.
- **AC-3** (bench, advisory) Given `fixtures/scan/poly/bench_1e8`, REQ-004 is measured and published.

## Design

- **API** (`fid-scan-poly::ooc`): `fn reconstruct(cloud: &dyn PointSource, valid, params: &SurfParams, budget: MemoryBudget) -> Result<Mesh, PolyError>`.
- **Algorithm:** pass 1 streams points (GEO.OOC.STREAM) to compute PREP cells per tile with halo via GEO.OOC.QUERY; pass 2 computes owned fans; vote records written as sorted runs and merged; STITCH on the merged votes.
- **Errors:** `scan.poly.budget_exceeded`, GEO.OOC errors passed through.
- **Determinism:** D0.
- **Crate:** `fid-scan-poly`.

## Test fixtures

`sphere_points` (shared), `bench_1e8` (generated at CI time), CC0.

## Build prompt

> In the Rust crate `fid-scan-poly`, module `ooc`, implement `reconstruct(cloud, valid, params, budget)`: estimate memory and either call `stitch::reconstruct_in_memory` or run the chunked path. Chunked: tiles aligned to the PREP grid in Morton order with a halo of 2h + g (neighbours via GEO.OOC `ChunkedIndex`); compute owned PREP cells and owned fans per tile; number vertices by cell Morton code; write fan votes as sorted runs and merge; stitch once. Mesh digests must equal the in-memory digest for any tile size and thread count. Constraints: KNP-1, §8, D0. Tests: AC-1..AC-3.

## Open questions

- None; this answers the parent's seam question by construction (local methods, halo, global vertex numbering).

## Children

_Leaf: none._
