---
id: GEO.OOC.CMD
title: Chunked cloud and LOD commands
level: 3
parent: GEO.OOC
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-GEO.OOC-001, REQ-GEO.OOC-006, REQ-GEO.OOC-007]
covers: []
depends_on: [GEO.OOC.LAYOUT, GEO.OOC.CACHE, GEO.OOC.LOD, GEO.TYPES.REG, CORE.REG, CORE.JOB]
side_effect: W2
determinism: D0
cli: [fid geo convert-chunks, fid geo verify-chunks, fid geo build-lod]
mcp: [geo_convert_chunks, geo_verify_chunks, geo_build_lod]
journal: [geo.convert_chunks@1, geo.verify_chunks@1, geo.build_lod@1]
updated: 2026-11-16
---

# GEO.OOC.CMD: Chunked cloud and LOD commands

## Purpose

The registry commands that convert a cloud between in-memory and chunked form, verify a chunked cloud's integrity and build its display LOD, run as cancellable CORE jobs.

## Scope

**In:**
- `geo.convert_chunks@1` (W2): in-memory cloud → chunked element, or chunked → in-memory element when it fits a memory budget
- `geo.verify_chunks@1` (R1): check every chunk digest and the manifest
- `geo.build_lod@1` (R1): build or rebuild the LOD into the cache store and return its statistics
- Progress and cancellation through CORE.JOB

**Out:**
- Import of LAS, LAZ, COPC and E57 into chunks (owned by `IO`)
- Library logic (owned by the other GEO.OOC children)

## Requirements

- **REQ-GEO.OOC.CMD-001** `geo.convert_chunks@1` shall be class W2, produce one journal entry, register the result as an element derived from the source, and keep the original element digest in the manifest.
- **REQ-GEO.OOC.CMD-002** `geo.verify_chunks@1` and `geo.build_lod@1` shall be class R1 and write no Record Journal entry.
- **REQ-GEO.OOC.CMD-003** When `geo.verify_chunks@1` finds a chunk whose bytes do not match its digest, the command shall report every failing cell and digest and return `integrity_failed`.
- **REQ-GEO.OOC.CMD-004** The three commands shall run as CORE jobs with progress and cancellation; a cancelled conversion shall leave no new element and no journal entry.
- **REQ-GEO.OOC.CMD-005** If a conversion to memory would exceed the given memory budget, then the command shall fail with `budget_exceeded` and the needed bytes.

## Acceptance criteria

- **AC-1** Given a project with `fixtures/geo/ooc/cloud_5e6` in memory, when `fid geo convert-chunks <id> --to chunked --json` runs, then a chunked element exists, the journal grows by one entry, and converting back yields the original digest.
- **AC-2** Given a chunked cloud with one corrupted chunk (fault-injecting store), when `fid geo verify-chunks <id> --json` runs, then it returns `integrity_failed` listing exactly that cell.
- **AC-3** Given the chunked cloud, when `fid geo build-lod <id> --json` runs, then statistics (nodes, points, bytes) are returned and the Record Journal is unchanged.
- **AC-4** (cancel) Given a conversion cancelled midway, then no element is created and the journal is unchanged.
- **AC-5** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `geo.convert_chunks@1` W2 `{ element, to: chunked | memory, max_chunk_points?, memory_budget_bytes? }` → new element ID; `geo.verify_chunks@1` R1 `{ element }` → `{ ok, failures: [{cell, digest}] }`; `geo.build_lod@1` R1 `{ element, seed?, node_budget? }` → LOD statistics.
- **Data structures:** schemas only.
- **Algorithm:** thin job wrappers over the library.
- **Errors:** `integrity_failed`, `budget_exceeded`, `cancelled`, `not_found`, `wrong_kind`.
- **Determinism:** D0.
- **Performance budget:** wrapper overhead negligible against the job [unverified].
- **Crate:** `fid-geo-cmd`.

## Test fixtures

- `cloud_5e6`; fault-injecting store from CORE's test harness.

## Build prompt

> In crate `fid-geo-cmd`, register three commands run as CORE jobs with progress and cancellation (CLI and MCP generated from the manifest): `geo.convert_chunks@1` (W2; in-memory ↔ chunked using `fid-geo-ooc::layout`, result registered as derived, original digest kept; cancelled runs leave nothing), `geo.verify_chunks@1` (R1; checks every chunk digest and the manifest, returns `integrity_failed` with all failing cells), `geo.build_lod@1` (R1; builds the LOD into the cache store and returns statistics). Errors: `integrity_failed`, `budget_exceeded` (with needed bytes), `cancelled`, `not_found`, `wrong_kind`. Acceptance: AC-1..AC-5 (conversion round trip with one journal entry; corruption reported; LOD build with no journal growth; cancellation leaves nothing; exact replay `IDENTICAL`). Constraints: CONSTITUTION §3 classes, §8 (no panic, no `unsafe`). Files: `crates/fid-geo-cmd/src/ooc_cmds.rs`, `tests/ooc_cmds.rs`.

## Open questions

- None.

## Children

_Leaf: none._
