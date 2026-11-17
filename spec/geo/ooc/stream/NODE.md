---
id: GEO.OOC.STREAM
title: Ordered full-resolution point streaming
level: 3
parent: GEO.OOC
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-GEO.OOC-002, REQ-GEO.OOC-005]
covers: []
depends_on: [GEO.OOC.CACHE, GEO.TYPES.ENV]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GEO.OOC.STREAM: Ordered full-resolution point streaming

## Purpose

The only way kernel operations read a chunked cloud: a stream of full-resolution point blocks in a fixed order that depends only on the cloud's content. It is a trait the kernel can consume without doing I/O itself, and the type system keeps LOD data out of it.

## Scope

**In:**
- Implementation of the `PointSource` trait (defined in `fid-geo-types` by GEO.TYPES.ENV) for chunked clouds
- Spatial order: chunks in Morton order, points within a chunk by ascending original index
- Original order: a k-way merge by original index across chunks, for operations and digests that need input order
- Block size control and cancellation checks between blocks

**Out:**
- LOD summaries (owned by `GEO.OOC.LOD`; they cannot be turned into a `PointSource`)
- Cross-chunk neighbour queries (owned by `GEO.OOC.QUERY`)

## Requirements

- **REQ-GEO.OOC.STREAM-001** A stream shall visit every point of the chunked cloud exactly once.
- **REQ-GEO.OOC.STREAM-002** The spatial-order stream shall emit chunks in Morton order of their cell codes and points within a chunk by ascending original index, so the order depends only on the manifest.
- **REQ-GEO.OOC.STREAM-003** The original-order stream shall emit points by ascending original index, and its element digest shall equal the manifest's original digest.
- **REQ-GEO.OOC.STREAM-004** `PointSource` shall be implemented only for full-resolution sources, and no conversion from LOD data to `PointSource` shall exist.
- **REQ-GEO.OOC.STREAM-005** Block boundaries shall not change the sequence of points emitted.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/ooc/cloud_5e6` chunked, when streamed in spatial order with block sizes 1 000 and 65 536, then both sequences are identical and contain each original index exactly once.
- **AC-2** Given the same cloud, when streamed in original order and digested, then the digest equals the manifest's original digest.
- **AC-3** Given the cloud chunked on two platforms, when streamed, then the spatial-order sequence digest is identical.
- **AC-4** A compile-fail test shows that `LodPoints` cannot be passed where `impl PointSource` is required.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `trait PointSource { fn next_block(&mut self) -> Option<Result<PointBlock, OocError>>; fn len(&self) -> u64; fn frame(&self) -> FrameRef; }`; `PointBlock { original_index: Vec<u64>, x, y, z, channels }`.
- **Algorithm:** spatial order walks the manifest list; original order uses a min-heap keyed by (original index) over per-chunk cursors, holding at most one pinned chunk per active cursor (bounded by a configurable fan-in with multi-pass merge when the fan-in is exceeded).
- **Errors:** propagated cache and store errors; `Cancelled`.
- **Determinism:** D0.
- **Performance budget:** spatial streaming at ≥ 200 M points/s when chunks are cached [unverified].
- **Crate:** `fid-geo-ooc` (the `PointSource` trait is defined in `fid-geo-types` by GEO.TYPES.ENV, so kernel crates consume it without depending on `fid-geo-ooc`).

## Test fixtures

- `cloud_5e6` from GEO.OOC.LAYOUT.

## Build prompt

> The trait `PointSource` already exists in `fid-geo-types` (`next_block`, `len`, `frame`; blocks of `original_index`, x, y, z and requested channels). In crate `fid-geo-ooc` module `stream`, implement it for chunked clouds in two orders: spatial (chunks in Morton order, points by original index) and original (k-way merge by original index with bounded fan-in and multi-pass merging). Use `ChunkCache` for loads; check a cancel flag between blocks. Ensure no conversion exists from `LodPoints` to `PointSource`. Acceptance: AC-1 identical sequences for two block sizes, each index once; AC-2 original-order digest equals manifest digest; AC-3 identical sequence digest across platforms; AC-4 compile-fail test for LOD. Constraints: CONSTITUTION §8 (no panic, no `unsafe`), C1 (kernel crates consume only the trait). Files: `crates/fid-geo-ooc/src/stream.rs`, `tests/stream.rs`.

## Open questions

- None.

## Children

_Leaf: none._
