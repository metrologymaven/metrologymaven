---
id: IO.CLOUD.STREAM
title: Streaming point sink and attribute selection for cloud readers and writers
level: 3
parent: IO.CLOUD
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.CLOUD-007, REQ-IO.CLOUD-008]
covers: []
depends_on: [IO.REG.ENV, IO.REG.LIMITS, GEO.OOC.LAYOUT, GEO.OOC.STREAM, GEO.TYPES.POINTS, CORE.STO.CAS]
side_effect: W2
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.CLOUD.STREAM: Streaming point sink and attribute selection

## Purpose

The shared plumbing between cloud readers and GEO's out-of-core chunked clouds: a `PointSink` that readers push batches into, which writes chunks to staged blobs with bounded resident memory, and a `PointSource` with an attribute-selection model that exporters pull from. Without it, every format would re-solve memory bounding and attribute mapping.

## Scope

**In:**
- `PointSink` (batch push, per-scan grouping, attribute columns, staged blobs committed only on success)
- `PointSource` (chunked read for exporters) and `AttributeSelection` (which per-point attributes to write, with type mapping and a report of attributes that the target format cannot hold)
- Resident-memory accounting against `IO.REG.LIMITS`
- An in-memory sink for small files and tests

**Out:**
- Chunk layout, LOD and octrees (GEO.OOC)
- Format parsing and writing (format siblings)

## Requirements

- **REQ-IO.CLOUD.STREAM-001** While importing, the sink shall keep resident memory for point data at or below a configurable cap (default 1 GiB) independent of file size, so that a 10^8-point import stays under 4 GiB peak resident memory [unverified until measured].
- **REQ-IO.CLOUD.STREAM-002** When an import fails or is cancelled, every chunk the sink staged shall be unreachable from the document and eligible for garbage collection.
- **REQ-IO.CLOUD.STREAM-003** When an export selects attributes, the source shall deliver exactly the selected attributes in the requested order, and any selected attribute the target format cannot represent shall fail the export with `attribute_unsupported` naming it.
- **REQ-IO.CLOUD.STREAM-004** Points pushed in one batch order shall be stored and later read back in the same order.

## Acceptance criteria

- **AC-1** Given a seeded generator of 10^8 points in 1,000-point batches with intensity and colour (`fixtures/io/cloud/stream/gen.rs`, seed 11) and a 1 GiB cap, when pushed into `PointSink` and committed, then peak resident memory stays under 4 GiB [unverified until measured] and the chunked cloud holds 10^8 points.
- **AC-2** Given 10^6 seeded points pushed in batches of varying size, when read back through `PointSource` in original order, then every coordinate and attribute equals the pushed value at the same index.
- **AC-3** Given an import cancelled after 40% of the batches, then the sink's staged blobs are not referenced by the document and a CORE.STO garbage-collection pass removes them.
- **AC-4** Given an `AttributeSelection` of `[intensity, colour]` for a target whose capability table lacks colour, then `attribute_unsupported{attribute: colour}` is returned before any byte is written.
- **AC-5** Given a selection `[colour, intensity]`, then the source yields columns in exactly that order.
- **AC-6** Given a file under 64 MiB of point data, then the in-memory sink is used and produces the same element digest as the chunked sink.

## Design

- **Registry commands:** none (library used by every cloud import and export command; their classes are W2 for import, W1 for export).
- **Data structures:** `PointSink { push(batch: PointBatch, scan: ScanKey), begin_scan(scan_meta), commit() -> StagedClouds, abort() }` with a chunked implementation over `GEO.OOC.LAYOUT` `ChunkWriter` and an in-memory one; `PointBatch { xyz: [Vec<f64>; 3], attrs: AttributeColumns }`; `PointSource { next_chunk(&mut self) -> Option<PointBatch> }` over `GEO.OOC.STREAM` original order; `AttributeSelection { columns: Vec<AttributeId> }`; `FormatCapabilities { supported: &[(AttributeId, AttributeType)] }`; `AttributeType` mapping table (f32, u16, u8×3, f64, i64).
- **Algorithm:** batches are appended to a per-scan `ChunkWriter`; resident bytes are charged to the `IO.REG.LIMITS` budget and the writer spills when over the cap; staged chunk blobs are written to CORE.STO as unreferenced blobs and become reachable only when the envelope's transaction commits; abort drops the staging handle.
- **Errors:** `limit_exceeded`, `attribute_unsupported`, `cancelled`.
- **Determinism:** D0 (order-preserving; no hashing of unordered maps).
- **Performance budget:** sink throughput ≥ 50 M points/s excluding parsing [unverified until measured].
- **Crate:** `fid-io-cloud`.

## Test fixtures

Seeded synthetic point generator under `fixtures/io/cloud/stream/` (CC-BY-4.0); capability tables of the format leaves.

## Build prompt

> In crate `fid-io-cloud`, implement `PointSink` (batch push per scan, attribute columns, chunked implementation on `fid-geo-ooc` `ChunkWriter` plus an in-memory implementation for sources under 64 MiB, resident memory charged to the `fid-io-reg` budget with a default 1 GiB cap, staged CORE.STO blobs made reachable only on the envelope's commit, abort on failure or cancellation), `PointSource` (chunked pull in original order over `GEO.OOC.STREAM`), and `AttributeSelection` with a `FormatCapabilities` check that fails `attribute_unsupported{attribute}` before any write. Preserve batch order exactly. CONSTITUTION §8: no panic, no unwrap; BTreeMap only. Tests AC-1..AC-6.

## Open questions

- Depends on the CORE.STO staged-blob lifecycle noted in `IO.REG.ENV` (garbage collection of unreferenced blobs, `CORE.STO.CAS`).

## Children

_Leaf: none._
