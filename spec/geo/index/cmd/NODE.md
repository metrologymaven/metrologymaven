---
id: GEO.INDEX.CMD
title: Index build and neighbour query commands
level: 3
parent: GEO.INDEX
kind: feature
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-GEO.INDEX-001, REQ-GEO.INDEX-006, REQ-GEO.INDEX-008]
covers: []
depends_on: [GEO.INDEX.BUDGET, GEO.TYPES.REG, CORE.REG]
side_effect: R1
determinism: D0
cli: [fid geo build-index, fid geo query-knn, fid geo query-radius, fid geo query-box]
mcp: [geo_build_index, geo_query_knn, geo_query_radius, geo_query_box]
journal: [geo.build_index@1, geo.query_knn@1, geo.query_radius@1, geo.query_box@1]
built_in: [fid-geo-cmd]
built_pr: 476
updated: 2026-11-22
---

# GEO.INDEX.CMD: Index build and neighbour query commands

## Purpose

Exposes index building and neighbour queries through the registry so scripts, the CLI and agents can inspect neighbourhoods without writing Rust, with bounded result sizes and no effect on the document.

## Scope

**In:**
- `geo.build_index@1` (R1): build or warm the cached index for an element, return the build report
- `geo.query_knn@1`, `geo.query_radius@1`, `geo.query_box@1` (R1)
- Query points given inline (at most 10 000 per call) or as a selection handle
- Result size cap and truncation flag

**Out:**
- Index algorithms and cache (owned by `GEO.INDEX.KDTREE`, `GEO.INDEX.BVH`, `GEO.INDEX.BUDGET`)
- Commands that use neighbours to compute values of record (owned by `KRN`, `ALN`, `SCAN`)

## Requirements

- **REQ-GEO.INDEX.CMD-001** All four commands shall be registered as class R1 with MCP projection exposed and shall write no Record Journal entry.
- **REQ-GEO.INDEX.CMD-002** When a query runs, the command shall return for each query point the neighbour indexes and distances in metres, in the order defined by GEO.INDEX.KDTREE.
- **REQ-GEO.INDEX.CMD-003** If a query would return more than 10⁶ neighbour entries in total, then the command shall return the first 10⁶ in query order with `truncated: true`.
- **REQ-GEO.INDEX.CMD-004** If inline query points exceed 10 000 or are given in a frame other than the element's, then the command shall fail with `too_many_queries` or `frame_mismatch`.
- **REQ-GEO.INDEX.CMD-005** `geo.build_index@1` shall accept a memory budget parameter and return `budget_exceeded` with the needed byte count when the build would exceed it.

## Acceptance criteria

- **AC-1** Given a project containing `fixtures/geo/index/uniform_1e5`, when `fid geo query-knn <id> --k 16 --points q.json --json` runs, then the output equals the library result for the same inputs and the Record Journal length is unchanged.
- **AC-2** Given radius queries returning 1.2 × 10⁶ entries, when run, then exactly 10⁶ entries are returned with `truncated: true`.
- **AC-3** (error path) Given a budget of 1 MiB for a 10⁵-point index, when `fid geo build-index` runs, then it fails with `budget_exceeded` and reports the needed bytes.
- **AC-4** (error path) Given query points tagged with another frame, when queried, then it fails with `frame_mismatch`.
- **AC-5** The generated manifest lists the four MCP tools as exposed.

## Design

- **Registry command(s):** `geo.build_index@1` R1 `{ element, kind: point|primitive, budget_bytes? }` → `BuildReport`; `geo.query_knn@1` R1 `{ element, points | selection, k }`; `geo.query_radius@1` R1 `{ element, points | selection, radius: AuthoredLength }`; `geo.query_box@1` R1 `{ element, min, max }`; all → `{ results: [[{index, distance_m}]], truncated }`.
- **Data structures:** command parameter and result schemas only.
- **Algorithm:** thin adapters over the library; radius given as an authored length converted by GEO.FRAME.UNITS.
- **Errors:** `too_many_queries`, `frame_mismatch`, `budget_exceeded`, `not_found`.
- **Determinism:** D0.
- **Performance budget:** adapter overhead under 1 ms per call excluding the query [unverified].
- **Crate:** `fid-geo-cmd`.

## Test fixtures

- `uniform_1e5` and a test project from CORE's harness.

## Build prompt

> In crate `fid-geo-cmd`, register four R1 commands (MCP exposed, Activity Log only, CLI/MCP generated from the manifest): `geo.build_index@1 {element, kind, budget_bytes?}` → build report; `geo.query_knn@1 {element, points|selection, k}`; `geo.query_radius@1 {element, points|selection, radius}` (radius as an authored length parsed by `fid-geo-frame::units`); `geo.query_box@1 {element, min, max}`. Results: per query point a list of `{index, distance_m}` in (distance, index) order from `fid-geo-index`; cap total entries at 10⁶ with `truncated: true`; reject more than 10 000 inline points (`too_many_queries`) and query points in another frame (`frame_mismatch`); budget failures return `budget_exceeded` with needed bytes. Acceptance: AC-1..AC-5 as specified (CLI output equals library output with no journal growth; truncation; budget error; frame error; four exposed tools). Constraints: CONSTITUTION §3 naming and classes, §8 (no panic, no `unsafe`). Files: `crates/fid-geo-cmd/src/index_cmds.rs`, `tests/index_cmds.rs`.

## Open questions

- None.

## Children

_Leaf: none._
