---
id: IO.CLOUD.LAS.COPC
title: COPC hierarchy reading and octree writing
level: 4
parent: IO.CLOUD.LAS
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.CLOUD.LAS-005]
covers: []
depends_on: [IO.CLOUD.LAS.LAZ, IO.CLOUD.LAS.QUANT]
uses_artifacts: [IO.CORP.MANIF]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.CLOUD.LAS.COPC: COPC hierarchy reading and octree writing

## Purpose

Reads COPC 1.0 files (LAZ 1.4 with an info VLR and an octree hierarchy of point chunks) node by node, and writes COPC by building an octree over the exported points and storing each node as a LAZ chunk, so the output is both a valid COPC file and a plain LAZ 1.4 file for readers that ignore the COPC records.

## Scope

**In:**
- COPC info VLR (centre, half-size, spacing, root hierarchy page) and hierarchy EVLR pages; recursive page loading with depth and page-count bounds
- Node-by-node point reading in hierarchy order, then reordering into file order for import so the point set order is stable
- Octree writer: node point limit (default 100,000), deterministic subdivision, each node one LAZ chunk, hierarchy pages written after the points
- Output that is also valid plain LAZ 1.4

**Out:**
- LAZ codec (`IO.CLOUD.LAS.LAZ`), quantisation (`IO.CLOUD.LAS.QUANT`)
- Display LOD (GEO.OOC.LOD; COPC's hierarchy is not reused as the display LOD)

## Requirements

- **REQ-IO.CLOUD.LAS.COPC-001** A COPC file written by the system shall be readable as plain LAZ 1.4 by a reader that ignores the COPC VLRs, yielding the same points.
- **REQ-IO.CLOUD.LAS.COPC-002** If hierarchy pages form a cycle, exceed depth 32 or reference a chunk outside the file, then reading shall fail with `malformed` naming the page offset.
- **REQ-IO.CLOUD.LAS.COPC-003** The sum of point counts over all hierarchy nodes shall equal the header point count, else reading shall fail with `truncated_file`.
- **REQ-IO.CLOUD.LAS.COPC-004** Writing the same points with the same options twice shall produce byte-identical files.

## Acceptance criteria

- **AC-1** Given 10^6 seeded points, when written as COPC and read with this reader and with the plain LAZ reader (`IO.CLOUD.LAS.LAZ`, ignoring COPC VLRs), then both yield the same multiset of points.
- **AC-2** Given each `accept` COPC entry of `corpus/las.toml`, then node point counts sum to the header count.
- **AC-3** (error path) Given `fixtures/io/cloud/las/copc/cyclic_pages.copc.laz`, then `malformed{page_offset}`.
- **AC-4** Given the same COPC export twice, then files are byte-identical.

## Design

- **Registry commands:** none (library).
- **Data structures:** `CopcInfo`; `HierarchyPage`; `NodeKey { depth, x, y, z }`; `OctreeBuilder { limit }` (points spilled per node through the cloud stream source, ordered by key then original index).
- **Algorithm:** read: BFS over pages with a visited set (BTreeSet of offsets); write: compute cube from the data AABB, assign points to the deepest node under the limit, sort by (NodeKey, original index), compress each node as a chunk.
- **Errors:** `malformed{page_offset}`, `truncated_file`, `limit_exceeded`.
- **Determinism:** D0.
- **Performance budget:** COPC write of 10^7 points in < 60 s [unverified until measured].
- **Crate:** `fid-io-las`.

## Test fixtures

Seeded points; public COPC samples in `corpus/las.toml` [unverified: licences]; hostile hierarchies under `fixtures/io/cloud/las/copc/` (CC-BY-4.0).

## Build prompt

> In crate `fid-io-las`, implement COPC 1.0: parse the info VLR and hierarchy EVLR pages (cycle, depth > 32 or out-of-file chunk → `malformed{page_offset}`; node counts must sum to the header count, else `truncated_file`), read points node by node and return them in file order; write COPC by building a deterministic octree (node limit 100,000, ordering by node key then original index), one LAZ chunk per node, then hierarchy pages, so the file is also valid plain LAZ 1.4. Byte-identical across runs. CONSTITUTION §8: no panic, no unwrap; BTreeMap/BTreeSet only. Tests AC-1..AC-4.

## Open questions

- None.

## Children

_Leaf: none._
