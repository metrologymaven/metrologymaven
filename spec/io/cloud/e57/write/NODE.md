---
id: IO.CLOUD.E57.WRITE
title: E57 export command with bit-exact f64 round trip
level: 4
parent: IO.CLOUD.E57
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.CLOUD.E57-005]
covers: [PW.EXP.01]
depends_on: [IO.CLOUD.E57.CVEC, IO.CLOUD.E57.XML, IO.CLOUD.STREAM, IO.REG.ENV, IO.REG.WRITE]
side_effect: W1
determinism: D0
cli: [fid export cloud-e57]
mcp: [export_cloud_e57]
journal: [export.cloud_e57@1]
updated: 2026-11-17
---

# IO.CLOUD.E57.WRITE: E57 export command

## Purpose

`export.cloud_e57@1` writes selected point sets as an E57 file, one scan per point set, with f64 Cartesian coordinates as stored and the point set's frame written as the scan pose, plus the selected attributes. Re-importing the file gives bit-identical coordinates and the same frames, which makes E57 the evidence-grade point exchange format.

## Scope

**In:**
- `export.cloud_e57@1` (W1, `fs_effect: write_root`): `point_sets`, `target`, `attributes?`, `relative_to?` (frame the poses are expressed in; default world), `overwrite?`, `dry_run?`
- Scan pose = the point set's frame expressed in `relative_to`; coordinates written as stored
- 64-bit float Cartesian fields; attributes via `AttributeSelection` (intensity as f32, colour as u8 with limits 0–255, row/column as integers)
- Deterministic file: scan GUIDs as UUIDv5 from the point set's element digest; XML and packets in a fixed order

**Out:**
- Spherical or scaled-integer output (not required)
- Encoding layers (`.PHYS`, `.XML`, `.CVEC`)

## Requirements

- **REQ-IO.CLOUD.E57.WRITE-001** When a point set with f64 coordinates is exported to E57 and re-imported, the coordinates shall be bit-identical and the re-imported frame shall equal the exported one within 1e-15 relative per matrix element.
- **REQ-IO.CLOUD.E57.WRITE-002** The exporter shall write exactly the selected attributes, and if one cannot be represented in E57, then the export shall fail with `attribute_unsupported` before writing.
- **REQ-IO.CLOUD.E57.WRITE-003** Exporting the same point sets with the same options twice shall give byte-identical files.

## Acceptance criteria

- **AC-1** Given 3 seeded point sets of 10^5 f64 points in 3 different frames, when `fid export cloud-e57 --point-sets ... --target <handle> --json` runs and the file is re-imported with `import.cloud_e57@1`, then coordinates are bit-identical and frames match within 1e-15 relative.
- **AC-2** Given `--attributes intensity,colour`, then the file's prototypes hold exactly those fields plus x,y,z.
- **AC-3** (error path) Given `--attributes normal_x` [unverified: whether E57 has a standard normal field], then `attribute_unsupported` and no file is written.
- **AC-4** Given the same export twice, then files are byte-identical.

## Design

- **Registry commands:** `export.cloud_e57@1` (W1, `fs_effect: write_root`; result `ExportReport`). CLI `fid export cloud-e57`, MCP `export_cloud_e57`.
- **Algorithm:** for each point set: pose = frame in `relative_to` as quaternion (fixed-order conversion via `fid-num`) + translation; pull `PointSource` batches → `CvecWriter`; then XML; then header; atomic write via `IO.REG.WRITE`.
- **Errors:** `attribute_unsupported`, `target_exists`, `raw_path_forbidden`, `path_outside_root`.
- **Determinism:** D0.
- **Performance budget:** 10^8 points in < 90 s [unverified until measured].
- **Crate:** `fid-io-cloud` (using `fid-io-e57`).

## Test fixtures

Seeded point sets; no external files.

## Build prompt

> In crate `fid-io-cloud`, implement `export.cloud_e57@1` (W1, `fs_effect: write_root`; CLI `fid export cloud-e57`; MCP `export_cloud_e57`) on the `fid-io-reg` export skeleton using `fid-io-e57`. One `data3D` per point set: pose = point-set frame relative to `relative_to` (quaternion + translation, `fid-num`), coordinates written as stored in 64-bit floats, selected attributes only (`attribute_unsupported` before writing), GUIDs UUIDv5 from the element digest, fixed ordering for byte-identical output, atomic write. Round-trip test with `import.cloud_e57@1` must be bit-identical. CONSTITUTION §8: no panic, no unwrap. Tests AC-1..AC-4.

## Open questions

- Normals are not a standard E57 point field [unverified]; an extension namespace could carry them later.

## Children

_Leaf: none._
