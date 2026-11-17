---
id: IO.CLOUD.LAS
title: LAS, LAZ and COPC reader and writer with scaled-integer precision
level: 3
parent: IO.CLOUD
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-IO.CLOUD-005, REQ-IO.CLOUD-008, REQ-IO.CLOUD-009]
covers: [PW.EXP.01]
depends_on: [IO.CLOUD.STREAM, IO.REG.ENV, IO.REG.WRITE]
cli: [fid import cloud-las, fid export cloud-las]
mcp: [import_cloud_las, export_cloud_las]
journal: [import.cloud_las@1, export.cloud_las@1]
updated: 2026-11-17
---

# IO.CLOUD.LAS: LAS, LAZ and COPC

## Purpose

Reads LAS 1.0 to 1.4, LAZ-compressed LAS and COPC 1.0, and writes LAS, LAZ and COPC. All three store coordinates as scaled 32-bit integers, so this node owns honest precision handling: it records each axis's scale and offset, attaches the quantisation uncertainty q/√12 per axis for UNC, and refuses to write a coordinate it cannot represent at the chosen scale.

## Scope

**In:**
- Public header block, VLRs and EVLRs (CRS WKT kept as untrusted metadata), point data record formats 0–10
- LAZ decompression and compression; COPC hierarchy reading (streamed by node)
- Scale/offset capture and quantisation metadata
- Writer: scale selection (≤ 1e-7 m per axis), offset selection, range check, COPC octree build
- `import.cloud_las@1` (W2) and `export.cloud_las@1` (W1 write_root), with `container: las|laz|copc`

**Out:**
- Geodetic CRS transformations (not in IO scope; CRS is metadata only)
- Streaming plumbing (`IO.CLOUD.STREAM`)

## Requirements

- **REQ-IO.CLOUD.LAS-001** When a LAS, LAZ or COPC file is imported, the system shall record each axis's scale and offset and attach quantisation uncertainty q/√12 per axis as point-set metadata.
- **REQ-IO.CLOUD.LAS-002** Imported coordinates shall be computed as offset + scale × integer in f64 with one rounding per axis.
- **REQ-IO.CLOUD.LAS-003** When the system writes LAS, LAZ or COPC, it shall use a scale of at most 1e-7 m per axis, and if any coordinate is not representable as a 32-bit integer at that scale and the chosen offset, then the export shall fail with `out_of_range` before writing.
- **REQ-IO.CLOUD.LAS-004** If the header's declared point count disagrees with the data length, then the import shall fail with `truncated_file`.
- **REQ-IO.CLOUD.LAS-005** A COPC file written by the system shall be readable as plain LAZ 1.4 by a reader that ignores the COPC VLRs.

## Interfaces

- **Offers:** `import.cloud_las@1` (W2), `export.cloud_las@1` (W1, `fs_effect: write_root`), quantisation metadata consumed by UNC.
- **Consumes:** `IO.CLOUD.STREAM`, `IO.REG` envelope and writer.

## Decomposition hints

HDR (headers, VLRs, PDRF), LAZ (codec binding), COPC (hierarchy read and octree write), QUANT (scale, offset, range, uncertainty metadata).

## Open questions

- LAZ codec: the concept docs name `laz-rs` [unverified: licence on the D-001 allowlist].
- Default export container for evidence (E57 versus COPC) is a TRUST/UNC question (carried from IO.CLOUD).
- REQ-IO.CLOUD.LAS-002 asks for "one rounding per axis"; KNP-1 forbids FMA, so `IO.CLOUD.LAS.QUANT` uses error-free transformations (Dekker product, two-sum) and rounds once in software.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| IO.CLOUD.LAS.HDR | LAS headers, VLRs and point data record formats (leaf) | 004 | none |
| IO.CLOUD.LAS.QUANT | Scaled-integer coordinates and quantisation (leaf) | 001 002 003 | none |
| IO.CLOUD.LAS.LAZ | LAZ chunked compression binding (leaf) | 004 | none |
| IO.CLOUD.LAS.COPC | COPC hierarchy reading and octree writing (leaf) | 005 | none |
| IO.CLOUD.LAS.CMD | LAS, LAZ and COPC import and export commands (leaf) | 001 003 004 | PW.EXP.01 |
