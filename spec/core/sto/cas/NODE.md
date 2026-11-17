---
id: CORE.STO.CAS
title: Content-addressed blob store with chunking and integrity
level: 3
parent: CORE.STO
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-CORE.STO-004, REQ-CORE.STO-005, REQ-CORE.STO-006]
covers: []
depends_on: [CORE.STO.LAYOUT]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# CORE.STO.CAS: Content-addressed blob store with chunking and integrity

## Purpose

Stores large opaque data (point clouds, meshes, video segments, transcripts, snapshots) once, addressed by the SHA-256 of its content. Objects larger than a chunk size are split into chunks, and a digest of the chunk manifest names the whole object, which gives deduplication, partial verification and streaming. Every read is checked against its address.

## Scope

**In:**
- `BlobStore::put/get/has/stream` addressed by SHA-256
- Deduplication of identical content
- Chunking of large objects, chunk manifest format, manifest digest as object name
- Integrity check on every read; typed integrity error
- Streaming reads and writes with bounded memory
- Fsync behaviour so that SQL can order blob writes before rows

**Out** (and which node owns it instead):
- Chunk content encoding for point data (Arrow IPC plus zstd per the concept docs; owned by `GEO.OOC`; CAS stores opaque chunks)
- Disposal of payloads at end of retention (owned by `TRUST`)
- Database index of blobs (owned by `CORE.STO.SQL`)

## Requirements

- **REQ-CORE.STO.CAS-001** The blob store shall address every blob by the SHA-256 of its content and shall store identical content once.
- **REQ-CORE.STO.CAS-002** If a blob read returns bytes whose SHA-256 differs from its address, then the blob store shall return a typed `integrity` error and shall not return the bytes.
- **REQ-CORE.STO.CAS-003** The blob store shall support objects of at least 50 GB by splitting them into chunks, and the SHA-256 of the canonical chunk manifest shall name the whole object [unverified that 50 GB covers the largest expected scans].
- **REQ-CORE.STO.CAS-004** The blob store shall stream reads and writes of chunked objects with memory use bounded by a fixed multiple of the chunk size, independent of object size.
- **REQ-CORE.STO.CAS-005** When `put` returns success with a durability flag, the blob's bytes shall have been flushed to stable storage.
- **REQ-CORE.STO.CAS-006** When a chunked object is read with a byte range, the blob store shall read and verify only the chunks that overlap that range.

## Interfaces

- **Offers:** `BlobStore` API, `BlobRef` (digest plus size), chunk manifest schema.
- **Consumes:** `CORE.STO.LAYOUT` (blob directory).

## Decomposition hints

PUT (single-blob put/get/has and integrity), CHUNK (chunking, manifest, streaming, range reads), FSYNC (durability behaviour per platform).

## Open questions

- Fixed-size vs content-defined chunking: content-defined chunking dedupes edited data better but costs CPU; decide in CHUNK.

## Children

_None yet: status `open`._
