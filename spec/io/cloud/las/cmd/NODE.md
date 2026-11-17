---
id: IO.CLOUD.LAS.CMD
title: LAS, LAZ and COPC import and export commands
level: 4
parent: IO.CLOUD.LAS
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.CLOUD.LAS-001, REQ-IO.CLOUD.LAS-003, REQ-IO.CLOUD.LAS-004]
covers: [PW.EXP.01]
depends_on: [IO.CLOUD.LAS.COPC, IO.CLOUD.LAS.QUANT, IO.CLOUD.STREAM, IO.REG.ENV, IO.REG.WRITE]
uses_artifacts: [IO.CORP.MANIF]
side_effect: [W2, W1]
determinism: D0
cli: [fid import cloud-las, fid export cloud-las]
mcp: [import_cloud_las, export_cloud_las]
journal: [import.cloud_las@1, export.cloud_las@1]
updated: 2026-11-17
---

# IO.CLOUD.LAS.CMD: LAS, LAZ and COPC commands

## Purpose

The user-facing commands for the LAS family: `import.cloud_las@1` reads LAS, LAZ or COPC (detected from the file) into one point set with its quantisation metadata, streaming through `PointSink`; `export.cloud_las@1` writes a point set as LAS, LAZ or COPC with the chosen container, honest scale and selected attributes.

## Scope

**In:**
- `import.cloud_las@1` (W2): `source`, `unit?`, `attributes?`, `stage?`, `dry_run?`, `limits?`
- `export.cloud_las@1` (W1, `fs_effect: write_root`): `point_set`, `target`, `container: las|laz|copc`, `scale?`, `attributes?`, `overwrite?`, `dry_run?`
- Format descriptors: LAS (`LASF` signature; `.las`), LAZ (`LASF` + LAZ VLR; `.laz`), COPC (LAZ + COPC info VLR; `.copc.laz`)
- Unit: from the CRS linear unit when declared and recognised, else `unit` required; CRS text kept as untrusted metadata, never used for transformation
- Attribute mapping to and from LAS fields (intensity, RGB, classification, GPS time, return info, extra bytes) through `AttributeSelection`
- Import and export reports: container, version, point format, scale/offset, quantisation uncertainty, attribute list

**Out:**
- Format layers (`.HDR`, `.QUANT`, `.LAZ`, `.COPC`)
- Geodetic CRS transformation (not in IO scope)

## Requirements

- **REQ-IO.CLOUD.LAS.CMD-001** When a LAS, LAZ or COPC file is imported, the command shall create one point set carrying per-axis scale, offset and quantisation uncertainty metadata and the selected attributes.
- **REQ-IO.CLOUD.LAS.CMD-002** If the file declares no recognised linear unit and no `unit` is given, then the import shall fail with `unit_required`.
- **REQ-IO.CLOUD.LAS.CMD-003** When exporting, the command shall apply the quantisation rules (scale ≤ 1e-7 m, range check before writing) and write the requested container.
- **REQ-IO.CLOUD.LAS.CMD-004** If a selected attribute has no LAS representation in the chosen point format, then the export shall fail with `attribute_unsupported` before writing.
- **REQ-IO.CLOUD.LAS.CMD-005** If the header's point count disagrees with the data, then the import shall fail with `truncated_file` and commit nothing.

## Acceptance criteria

- **AC-1** Given `fixtures/io/cloud/las/cmd/small_mm.las` (1,000 points, scale 0.001, CRS linear unit metre), when `fid import cloud-las --source <handle> --json` runs, then one point set exists with metadata scale 0.001, `u_quant` 0.001/√12 per axis, and one journal event.
- **AC-2** Given the same points exported with `--container laz` and `--container copc`, when each is re-imported, then coordinates agree with the originals within the reported max quantisation error.
- **AC-3** (error path) Given `fixtures/io/cloud/las/cmd/no_crs.las` without `--unit`, then `unit_required`.
- **AC-4** (error path) Given a 500 m point set exported with defaults, then `out_of_range` and no file exists.
- **AC-5** (error path) Given `fixtures/io/cloud/las/hdr/short_points.las`, then `truncated_file` and nothing is journaled.
- **AC-6** Given every `accept` entry of `corpus/las.toml`, then counts match; every `reject` entry returns its expected error.
- **AC-7** (replay) Given the journal of AC-1, when replayed exactly, then `IDENTICAL`.

## Design

- **Registry commands:** `import.cloud_las@1` (W2) and `export.cloud_las@1` (W1, `fs_effect: write_root`); side_effect list `[W2, W1]` in cli/mcp order. CLI `fid import cloud-las`, `fid export cloud-las`; MCP `import_cloud_las`, `export_cloud_las`.
- **Algorithm:** import: detect container → open layers → stream batches → QUANT conversion → `PointSink` → one transaction; export: `PointSource` → QUANT plan and range check → write container → atomic write.
- **Errors:** `unit_required`, `truncated_file`, `malformed`, `out_of_range`, `scale_too_coarse`, `attribute_unsupported`, `limit_exceeded`, envelope errors.
- **Determinism:** D0.
- **Performance budget:** 10^8-point LAZ import in < 180 s and < 4 GiB peak resident memory [unverified until measured].
- **Crate:** `fid-io-cloud` (using `fid-io-las`).

## Test fixtures

Project-generated files under `fixtures/io/cloud/las/cmd/` (CC-BY-4.0); `corpus/las.toml` (`IO.CORP.MANIF`).

## Build prompt

> In crate `fid-io-cloud`, implement `import.cloud_las@1` (W2; CLI `fid import cloud-las`; MCP `import_cloud_las`) and `export.cloud_las@1` (W1, `fs_effect: write_root`; CLI `fid export cloud-las`; MCP `export_cloud_las`) on the `fid-io-reg` skeleton using `fid-io-las`. Register LAS, LAZ and COPC descriptors. Import: detect container, stream raw batches, convert with the QUANT rules (one rounding), attach scale/offset/`u_quant` metadata, map attributes, take the unit from a recognised CRS linear unit else require `unit`, keep CRS as untrusted metadata, commit in one transaction. Export: container `las|laz|copc`, scale ≤ 1e-7 m (default 1e-7), range check before writing (`out_of_range`), `attribute_unsupported` before writing, atomic write, report scale/offset/max error. CONSTITUTION §8: no panic, no unwrap. Tests AC-1..AC-7.

## Open questions

- Which CRS linear-unit encodings (WKT, GeoKeys) to recognise [unverified]; others require `unit`.

## Children

_Leaf: none._
