---
id: IO.CLOUD.E57.PHYS
title: E57 file header, paged physical layer and page checksums
level: 4
parent: IO.CLOUD.E57
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
built_in: [fid-io-e57]
satisfies: [REQ-IO.CLOUD.E57-004]
covers: []
depends_on: [IO.REG.LIMITS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.CLOUD.E57.PHYS: E57 file header and paged physical layer

## Purpose

The bottom layer of the E57 reader and writer: the fixed file header and the paged physical layout in which every page ends with a checksum. It turns physical offsets into a logical byte stream (and back), verifies every page checksum on read, and writes pages with correct checksums. Everything above it works on logical bytes.

## Scope

**In:**
- File header: signature, major/minor version, physical file length, XML section offset and length, page size [unverified: exact field list and order to confirm against ASTM E2807]
- Page model: fixed page size with a trailing CRC-32C per page [unverified: checksum algorithm]
- `LogicalReader`: seek and read logical ranges, verifying each page's checksum once, with an LRU of verified pages bounded in bytes
- `LogicalWriter`: append logical bytes, emitting full pages with checksums, and patching the header at finish
- Bounds: header lengths checked against the actual file length before any read

**Out:**
- XML (`IO.CLOUD.E57.XML`), binary sections and codecs (`IO.CLOUD.E57.CVEC`)

## Requirements

- **REQ-IO.CLOUD.E57.PHYS-001** If a page checksum does not match, then reading shall fail with `checksum_mismatch` naming the page's physical offset.
- **REQ-IO.CLOUD.E57.PHYS-002** If the header signature is wrong, or the declared file length, XML offset or XML length is inconsistent with the actual file length, then opening shall fail with `malformed` naming the field.
- **REQ-IO.CLOUD.E57.PHYS-003** Bytes written through `LogicalWriter` shall read back through `LogicalReader` identically, with every page checksum valid.
- **REQ-IO.CLOUD.E57.PHYS-004** The verified-page cache shall never exceed its byte budget (default 16 MiB).

## Acceptance criteria

- **AC-1** Given 10 MiB of seeded logical bytes written with `LogicalWriter`, when read back with `LogicalReader` in random seeded ranges, then all ranges match and no checksum fails.
- **AC-2** (error path) Given the file of AC-1 with one byte flipped in page 7, then reading a range covering page 7 fails `checksum_mismatch{offset: 7 × page_size}`.
- **AC-3** (error path) Given a header declaring a file length larger than the file, then `malformed{field: file_length}`.
- **AC-4** Given a public sample E57 file from the corpus (`corpus/e57.toml`), then every page checksum verifies.

## Design

- **Registry commands:** none (library).
- **Data structures:** `E57Header`; `LogicalReader<R: Read + Seek>`; `LogicalWriter<W: Write + Seek>`; `PageCache { budget_bytes }` keyed by page index in a `BTreeMap` with an access counter for eviction.
- **Algorithm:** logical offset l → physical p = (l / payload) × page_size + l mod payload, payload = page_size − 4; CRC-32C over each page payload.
- **Errors:** `checksum_mismatch{offset}`, `malformed{field}`, `truncated_file`, `limit_exceeded`.
- **Determinism:** D0.
- **Performance budget:** ≥ 1 GB/s checksum verification [unverified until measured].
- **Crate:** `fid-io-e57` (first-party pure Rust; the C++ libE57Format is not used because CONSTITUTION §8 forbids `unsafe` FFI without a named exception).

## Test fixtures

Seeded round-trip data; public E57 sample files listed in `corpus/e57.toml` [unverified: licences].

## Build prompt

> Create crate `fid-io-e57` (pure Rust, no FFI). Implement the E57 file header and paged physical layer: parse and validate the header (signature, version, lengths and offsets against the real file length → `malformed{field}`), map logical to physical offsets for the declared page size with a trailing CRC-32C per page, verify each page once on read (`checksum_mismatch{offset}`) with a byte-bounded verified-page cache (16 MiB default), and write pages with checksums, patching the header at finish. Confirm field layout and checksum algorithm against ASTM E2807 by designator only; no standards text. CONSTITUTION §8: no panic, no unwrap; BTreeMap only. Tests AC-1..AC-4.

## Open questions

- Header field layout and checksum algorithm to confirm against ASTM E2807 [unverified].

## Children

_Leaf: none._
