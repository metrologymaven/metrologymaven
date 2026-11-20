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
built_in: [fid-store]
built_pr: 125
satisfies: [REQ-CORE.STO-004, REQ-CORE.STO-005, REQ-CORE.STO-006]
covers: []
depends_on: [CORE.STO.LAYOUT, CORE.JRN.CANON]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-20
---

# CORE.STO.CAS: Content-addressed blob store with chunking and integrity

## Purpose

Stores large opaque data (point clouds, meshes, video segments, transcripts, observation records) once, addressed by the SHA-256 of its content. Objects larger than one chunk are split into fixed-size chunks, and the SHA-256 of a canonical chunk manifest names the whole object, which gives deduplication, partial verification and bounded-memory streaming. Every read is checked against its address, and a durable put returns only after the bytes are on stable storage, so `CORE.STO.SQL.TXN` can order blob flushes before the rows that reference them.

## Scope

**In:** `BlobStore::put/get/has/stream/read_range/read_object`; single-blob addressing and deduplication; fixed-size chunking, the chunk manifest format `fid-chunks@1` and object naming; integrity checks on every read; streaming with bounded memory; durable put (file and directory flush per platform); the on-disk fan-out under the blob directory fixed by `CORE.STO.LAYOUT`.
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
- **REQ-CORE.STO.CAS-010** When content is read by digest alone with a length bound (`read_object`), the blob store shall return the content the digest names (the single blob, or the chunks of the object whose manifest it is, each verified against the manifest) and never the bytes of a chunk manifest, shall refuse content longer than the bound with `out_of_range` before allocating its buffer or reading any chunk, and shall return no bytes when any part cannot be verified.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/sto/cas/blobs.json` (100 seeded random blobs from 0 B to 20 MiB, with expected addresses from an independent SHA-256 tool), when put and read back, then every address matches and every read returns the original bytes.
- **AC-2** Given the same 1 MiB content put twice, then both puts return the same address and the blob directory holds one file for it.
- **AC-3** (error path) Given a stored blob with one byte flipped on disk, when read, then the result is `integrity` and no bytes are returned.
- **AC-4** Given a 20 MiB object, then it is stored as three chunks (8, 8 and 4 MiB) and its name equals the SHA-256 of the canonical manifest in `fixtures/core/sto/cas/manifest_20mib.json`.
- **AC-5** Given a synthetic 50 GB object streamed from a seeded generator (nightly job), when written and read back with a memory probe, then peak buffer memory stays within 2 chunks plus 1 MiB and the streamed digest matches.
- **AC-6** Given a 64 MiB object, when bytes 20,000,000..20,000,100 are read, then exactly one chunk is read and verified.
- **AC-7** (error path) Given a manifest listing 2^20 + 1 chunks, then reading it fails with `manifest_invalid`.
- **AC-8** Given every chunked object of `fixtures/core/sto/cas/objects.json` (8 MiB + 1 B to 20 MiB), when read with `read_object(name, size)`, then the SHA-256 of the result equals the fixture's `content` digest and the result is not the manifest. (error path) Given a tampered or missing chunk, then the result is `integrity` or `not_found` and no bytes; given `max_len` below the object size, then `out_of_range` with no chunk read; given a planted manifest whose chunks are absent, then it is refused, never returned.

## Design

- **Registry command(s):** none; library with no command of its own (`side_effect: R0` follows the convention of `CORE.STO.CACHE`). Its writes stay inside the project's blob directory and are never journaled by themselves; the commit that references a blob is.
- **Read by digest (REQ-010):** `read_object(digest, max_len) -> Vec<u8>` is the API for every consumer that holds only a digest (egress, stage switch). A blob that parses as a `fid-chunks@1` manifest within the object limits is read as the object it names: each chunk checked for its listed length and digest, then concatenated; content that is itself a valid manifest is therefore always read as an object, and such bytes stored as plain data are reachable only through the low-level read. Otherwise the blob is returned, verified. `max_len` is compared with the blob length, or with the manifest's `size`, before the result buffer is allocated; only a manifest (at most about 2.6 MiB) is read first. `get` with a blob address stays the low-level read of one stored blob and is documented to return a manifest's bytes for an object's name.
- **Data structures:** `BlobRef { digest, size }`; `ObjectRef { manifest_digest, size }`; `ChunkManifest { format: "fid-chunks@1", size, chunk_size, chunks: Vec<Digest> }` serialised with `fid-canon` (RFC 8785); `PutOpts { durable: bool }`.
- **Algorithm:** hash while writing to `tmp/<random>`; if the address exists, delete the temp file; else flush (when durable) and rename to `blobs/sha256/<h0h1>/<h2h3>/<hex>`, then flush the directory. Platform flush: `fsync` plus directory `fsync` on Linux; `F_FULLFSYNC` on macOS [unverified that it is required]; `FlushFileBuffers` on Windows. Fixed-size chunking was chosen over content-defined chunking: it is simpler and deterministic, and the manifest's `chunk_size` and format version leave room for content-defined chunking later.
- **Errors:** `integrity`, `not_found`, `manifest_invalid`, `out_of_range` (a range past the end, or `read_object` content longer than `max_len`: `start` 0, `end` the content size, `size` the bound), `io` (with OS error kind).
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
- Journal-referenced blobs as permanent GC roots: `UNC.MODEL.GRAPH.STORE` journals `unc.create_model@1` events that name the model blob (address and size) instead of carrying the bytes, so every such blob must stay reachable for as long as the journal entry exists; any future garbage collection must treat blobs named by journal events as roots (via the `CORE.STO.SQL.SCHEMA` blob reference index or a journal scan).

- **Stage switch not migrated yet.** #354 (`CORE.PRJ.STAGE.SWITCH`, `fid-project-cmd`) was open when `read_object` was added, so it still reads blobs by digest with its own manifest refusal; it should call `BlobStore::read_object` once both are on main. Egress (`CORE.BUS.POL.EGRESS`) is migrated.

## Change log

- REQ-010 and AC-8 added 2026-11-20 (reviews of #336 and #354): reading a blob by digest returned a chunk manifest's bytes for an object's name, and consumers served or sent the manifest as if it were the object. `read_object(digest, max_len)` returns the object's verified bytes or refuses; `get` with a blob address stays the low-level read and is documented as such.

## Children

_Leaf: none._
