---
id: IO.CLOUD.LAS.LAZ
title: LAZ chunked compression and decompression binding
level: 4
parent: IO.CLOUD.LAS
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-IO.CLOUD.LAS-004]
covers: []
depends_on: [IO.CLOUD.LAS.HDR, IO.REG.LIMITS]
uses_artifacts: [IO.CORP.MANIF]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.CLOUD.LAS.LAZ: LAZ chunked compression binding

## Purpose

Adds LAZ to the LAS layer: reads the LAZ VLR, decompresses point chunks one at a time into the same raw point batches the LAS reader yields, and compresses batches into chunks for the writer. It binds a pure-Rust LAZ codec (the concept docs name `laz-rs` [unverified: licence on the D-001 allowlist]) behind a bounded interface.

## Scope

**In:**
- LAZ VLR parsing (compressor type, chunk size, item list) with bounds
- Chunk table reading and validation (chunk offsets inside the file, point counts summing to the header count)
- Streaming decompression chunk by chunk into `RawPointBatch`; compression of batches with a fixed chunk size (default 50,000 points)
- Decompressed-size check per chunk against the declared point count × record length

**Out:**
- The codec's internals (third-party crate), headers and records (`IO.CLOUD.LAS.HDR`), COPC hierarchy (`IO.CLOUD.LAS.COPC`)

## Requirements

- **REQ-IO.CLOUD.LAS.LAZ-001** If the chunk table's point counts do not sum to the header's point count, or a chunk offset lies outside the file, then reading shall fail with `truncated_file` or `malformed` naming the chunk index.
- **REQ-IO.CLOUD.LAS.LAZ-002** If a chunk decompresses to a different number of bytes than its declared points × record length, then reading shall fail with `malformed{chunk}` and no point from that chunk shall be delivered.
- **REQ-IO.CLOUD.LAS.LAZ-003** Points compressed by the writer and decompressed by the reader shall be byte-identical to the input records.
- **REQ-IO.CLOUD.LAS.LAZ-004** Decompression memory shall be bounded per chunk, independent of file size.

## Acceptance criteria

- **AC-1** Given 10^6 seeded format-6 records, when compressed and decompressed, then the records are byte-identical.
- **AC-2** Given each `accept` LAZ entry of `corpus/las.toml` (`IO.CORP.MANIF`), then the decompressed point count matches the header and the manifest.
- **AC-3** (error path) Given `fixtures/io/cloud/las/laz/bad_chunk_table.laz`, then `malformed{chunk: 2}`.
- **AC-4** (error path) Given a chunk truncated by 100 bytes, then the read fails at that chunk and earlier chunks' points are not committed by the caller (the envelope discards them).

## Design

- **Registry commands:** none (library).
- **Data structures:** `LazVlr`; `ChunkTable { entries: Vec<(offset, points)> }`; `LazReader`/`LazWriter` implementing the LAS layer's batch traits.
- **Algorithm:** validate chunk table first; decompress one chunk into a buffer sized from the declared count (charged to the `Budget`).
- **Errors:** `malformed{chunk}`, `truncated_file`, `limit_exceeded`, `laz_compressor_unsupported`.
- **Determinism:** D0 (codec output depends only on input and chunk size [unverified for the chosen crate; checked by AC-1 digests]).
- **Performance budget:** ≥ 10 M points/s decompression [unverified until measured].
- **Crate:** `fid-io-las`.

## Test fixtures

Seeded round trips; public LAZ samples in `corpus/las.toml` [unverified: licences]; hostile chunk tables under `fixtures/io/cloud/las/laz/` (CC-BY-4.0).

## Build prompt

> In crate `fid-io-las`, bind a pure-Rust LAZ codec (check it is on the D-001 allowlist): parse the LAZ VLR and chunk table, validate offsets and point counts (`malformed{chunk}` / `truncated_file`), decompress chunk by chunk into raw point batches with size checks and bounded memory, and compress batches with a fixed chunk size for the writer. Round trip must be byte-identical. CONSTITUTION §8: no panic, no unwrap; no `unsafe` in our code. Tests AC-1..AC-4.

## Open questions

- Codec crate licence and determinism across platforms [unverified].

## Children

_Leaf: none._
