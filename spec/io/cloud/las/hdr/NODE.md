---
id: IO.CLOUD.LAS.HDR
title: LAS headers, VLRs and point data record formats 0 to 10
level: 4
parent: IO.CLOUD.LAS
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
built_in: [fid-io-las]
built_pr: 177
satisfies: [REQ-IO.CLOUD.LAS-004]
covers: []
depends_on: [IO.REG.LIMITS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.CLOUD.LAS.HDR: LAS headers, VLRs and point data record formats

## Purpose

Reads and writes the uncompressed LAS structure for versions 1.0 to 1.4: the public header block, variable-length and extended variable-length records, and fixed-size point records in formats 0 to 10 with extra bytes. It checks every declared size and count against the real file before trusting it, and is the base that the LAZ and COPC layers wrap.

## Scope

**In:**
- Public header block per version (signature, version, header size, offset to point data, VLR count, point format and record length, legacy and 64-bit point counts, scale and offset per axis, min/max per axis, EVLR offset and count)
- VLRs and EVLRs: user ID, record ID, length, payload bounded; CRS records (WKT and GeoKeys) kept as untrusted metadata; extra-bytes descriptors
- Point records formats 0–10: integer X, Y, Z, intensity, return fields, classification and flags, scan angle, user data, point source ID, GPS time, RGB, NIR, wave packet fields (counted, not imported), extra bytes as typed attributes
- Writer for header, VLRs and point records (format 6 or 7 by default for 1.4 output)

**Out:**
- Scale and offset arithmetic and quantisation (`IO.CLOUD.LAS.QUANT`)
- Compression (`IO.CLOUD.LAS.LAZ`), COPC hierarchy (`IO.CLOUD.LAS.COPC`)

## Requirements

- **REQ-IO.CLOUD.LAS.HDR-001** If the declared point count × record length does not equal the bytes between the point-data offset and the first EVLR (or end of file), then reading shall fail with `truncated_file` stating both numbers.
- **REQ-IO.CLOUD.LAS.HDR-002** If the header size, point-data offset, VLR lengths or EVLR offsets point outside the file or overlap, then reading shall fail with `malformed` naming the field.
- **REQ-IO.CLOUD.LAS.HDR-003** The reader shall decode point formats 0–10 into integer coordinates and typed attribute columns, and shall count but not import waveform data.
- **REQ-IO.CLOUD.LAS.HDR-004** Records written by the writer and read back shall be byte-identical in every field.
- **REQ-IO.CLOUD.LAS.HDR-005** VLR and EVLR payloads shall be bounded (default 64 MiB each) and returned as untrusted bytes or text.

## Acceptance criteria

- **AC-1** Given `fixtures/io/cloud/las/hdr/pdrf_all.zip` (one small file per format 0–10, project-generated), when read, then each yields the expected point count and attribute set.
- **AC-2** (error path) Given `fixtures/io/cloud/las/hdr/short_points.las` (header says 1,000 points, data holds 999), then `truncated_file{declared: 1000, actual: 999}`.
- **AC-3** (error path) Given a VLR length running past the point-data offset, then `malformed{field: vlr_length}`.
- **AC-4** Given 10^5 seeded format-7 records written and read back, then all fields are byte-identical.
- **AC-5** Given the LAS fuzz target run for its CI budget, then no panic occurs.

## Design

- **Registry commands:** none (library).
- **Data structures:** `LasHeader`; `Vlr { user_id: Untrusted, record_id: u16, payload: Bounded<Vec<u8>> }`; `PointFormat(u8)`; `RawPointBatch { xi: Vec<i32>, yi: Vec<i32>, zi: Vec<i32>, attrs: AttributeColumns }`.
- **Algorithm:** read header by version; validate all offsets against file length; stream records in batches of 65,536.
- **Errors:** `truncated_file`, `malformed{field}`, `limit_exceeded`, `las_version_unsupported`.
- **Determinism:** D0.
- **Performance budget:** ≥ 500 MB/s uncompressed read [unverified until measured].
- **Crate:** `fid-io-las`.

## Test fixtures

Project-generated LAS files under `fixtures/io/cloud/las/hdr/` (CC-BY-4.0).

## Build prompt

> Create crate `fid-io-las` (pure Rust). Read and write LAS 1.0–1.4: public header per version, VLRs and EVLRs (bounded payloads, untrusted), point formats 0–10 with extra bytes, waveform data counted not imported. Validate every offset and length against the real file length (`malformed{field}`); declared count × record length must match the data bytes (`truncated_file{declared, actual}`). Stream records in batches of 65,536 as integer coordinates plus typed attributes. Writer emits header, VLRs and records byte-exactly. No standards text; designators only. CONSTITUTION §8: no panic, no unwrap; BTreeMap only. Fuzz target. Tests AC-1..AC-5.

## Open questions

- None.

## Children

_Leaf: none._
