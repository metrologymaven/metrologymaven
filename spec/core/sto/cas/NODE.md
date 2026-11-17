---
id: CORE.STO.CAS
title: Content-addressed blob store with chunking and integrity
level: 3
parent: CORE.STO
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.STO-004, REQ-CORE.STO-005, REQ-CORE.STO-006]
covers: []
depends_on: [CORE.STO.LAYOUT, CORE.JRN.CANON]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.STO.CAS: Content-addressed blob store with chunking and integrity

## Purpose

Stores large opaque data (point clouds, meshes, video segments, transcripts, observation records) once, addressed by the SHA-256 of its content. Objects larger than one chunk are split into fixed-size chunks, and the SHA-256 of a canonical chunk manifest names the whole object, which gives deduplication, partial verification and bounded-memory streaming. Every read is checked against its address, and a durable put returns only after the bytes are on stable storage, so `CORE.STO.SQL.TXN` can order blob flushes before the rows that reference them.

## Scope

**In:** `BlobStore::put/get/has/stream/read_range`; single-blob addressing and deduplication; fixed-size chunking, the chunk manifest format `fid-chunks@1` and object naming; integrity checks on every read; streaming with bounded memory; durable put (file and directory flush per platform); the on-disk fan-out under the blob directory fixed by `CORE.STO.LAYOUT`.
**Out:** chunk content encoding for point data (Arrow IPC plus zstd per the concept docs; owned by `GEO.OOC.LAYOUT`, which stores its chunks here as opaque blobs); disposal of payloads at end of retention (`TRUST`); the database index of blobs and references (`CORE.STO.SQL.SCHEMA`); transaction ordering (`CORE.STO.SQL.TXN`).

## Requirements

- **REQ-CORE.STO.CAS-001** The blob store shall address every blob by the SHA-256 of its content, written as `sha256:` plus 64 lowercase hex digits, and when content already present is put again, it shall return the existing address without writing a second copy.
- **REQ-CORE.STO.CAS-002** If a blob read returns bytes whose SHA-256 differs from its address, then the blob store shall return a typed `integrity` error naming the address and shall not return the bytes.
- **REQ-CORE.STO.CAS-003** The blob store shall split any object larger than 8 MiB into 8 MiB chunks (the last may be shorter), store each chunk as a blob, and name the object by the SHA-256 of its canonical `fid-chunks@1` manifest, which lists total size, chunk size and the ordered chunk addresses.
- **REQ-CORE.STO.CAS-004** The blob store shall support objects of at least 50 GB [unverified that 50 GB covers the largest expected scans].
- **REQ-CORE.STO.CAS-005** Streaming reads and writes of chunked objects shall use at most two chunks plus 1 MiB of buffer memory, independent of object size.
- **REQ-CORE.STO.CAS-006** When `put` returns success with the durable flag, the blob's file and its directory entry shall have been flushed to stable storage with the platform's strongest ordinary flush call.
- **REQ-CORE.STO.CAS-007** When a chunked object is read with a byte range, the blob store shall read and verify only the chunks that overlap that range.
- **REQ-CORE.STO.CAS-008** Writes shall go to a temporary file in the blob directory followed by an atomic rename, so a crash never leaves a partially written file under a blob address.
- **REQ-CORE.STO.CAS-009** If a chunk manifest is larger than 16 MiB, lists more than 2^20 chunks, or its sizes disagree with its chunk list, then reading it shall fail with `manifest_invalid` without panicking.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/sto/cas/blobs.json` (100 seeded random blobs from 0 B to 20 MiB, with expected addresses from an independent SHA-256 tool), when put and read back, then every address matches and every read returns the original bytes.
- **AC-2** Given the same 1 MiB content put twice, then both puts return the same address and the blob directory holds one file for it.
- **AC-3** (error path) Given a stored blob with one byte flipped on disk, when read, then the result is `integrity` and no bytes are returned.
- **AC-4** Given a 20 MiB object, then it is stored as three chunks (8, 8 and 4 MiB) and its name equals the SHA-256 of the canonical manifest in `fixtures/core/sto/cas/manifest_20mib.json`.
- **AC-5** Given a synthetic 50 GB object streamed from a seeded generator (nightly job), when written and read back with a memory probe, then peak buffer memory stays within 2 chunks plus 1 MiB and the streamed digest matches.
- **AC-6** Given a 64 MiB object, when bytes 20,000,000..20,000,100 are read, then exactly one chunk is read and verified.
- **AC-7** (error path) Given a manifest listing 2^20 + 1 chunks, then reading it fails with `manifest_invalid`.

## Design

- **Registry command(s):** none; library with no command of its own (`side_effect: R0` follows the convention of `CORE.STO.CACHE`). Its writes stay inside the project's blob directory and are never journaled by themselves; the commit that references a blob is.
- **Data structures:** `BlobRef { digest, size }`; `ObjectRef { manifest_digest, size }`; `ChunkManifest { format: "fid-chunks@1", size, chunk_size, chunks: Vec<Digest> }` serialised with `fid-canon` (RFC 8785); `PutOpts { durable: bool }`.
- **Algorithm:** hash while writing to `tmp/<random>`; if the address exists, delete the temp file; else flush (when durable) and rename to `blobs/sha256/<h0h1>/<h2h3>/<hex>`, then flush the directory. Platform flush: `fsync` plus directory `fsync` on Linux; `F_FULLFSYNC` on macOS [unverified that it is required]; `FlushFileBuffers` on Windows. Fixed-size chunking was chosen over content-defined chunking: it is simpler and deterministic, and the manifest's `chunk_size` and format version leave room for content-defined chunking later.
- **Errors:** `integrity`, `not_found`, `manifest_invalid`, `io` (with OS error kind).
- **Determinism:** D0; addresses and manifests depend only on content.
- **Performance budget:** sustained put of 1 GB in < 5 s on reference SSD [unverified until measured].
- **Crate:** `fid-store` (module `cas`).

## Test fixtures

Seeded random content generated in tests; expected digests in `fixtures/core/sto/cas/` produced by an independent SHA-256 implementation script. The 50 GB case runs nightly only. CC-BY-4.0.

## Build prompt

> In crate `fid-store`, add module `cas`: `BlobStore` over the blob directory given by the `layout` module, with `put(reader, PutOpts{durable}) -> BlobRef|ObjectRef`, `get`, `has`, `stream`, `read_range`. Address = SHA-256 of content (`sha256:` + lowercase hex); dedupe on existing address. Objects over 8 MiB become 8 MiB chunks plus a canonical `fid-chunks@1` manifest (size, chunk_size, ordered chunk digests, serialised with `fid-canon`) whose SHA-256 names the object. Write to a temp file, flush when durable (Linux fsync + dir fsync; macOS F_FULLFSYNC; Windows FlushFileBuffers), atomic rename into `blobs/sha256/<2>/<2>/<hex>`. Verify every read; on mismatch return `integrity` and no bytes. Range reads touch only overlapping chunks. Memory ≤ 2 chunks + 1 MiB when streaming. Bound manifests (16 MiB, 2^20 chunks). Tests AC-1..AC-7 (AC-5 nightly). Constraints: CONSTITUTION §6 (SHA-256), §8 (no panic, no `unsafe`, hostile input), REQ-CORE.STO-005.

## Open questions

- **Resolved:** fixed-size versus content-defined chunking: fixed 8 MiB for P0 (see Design); revisit with measurements from `GEO.OOC` edited-cloud workloads.
- Garbage collection of unreferenced blobs is not specified anywhere in STO; it needs the blob reference index of `CORE.STO.SQL.SCHEMA` and retention rules from `TRUST`.

## Children

_Leaf: none._
