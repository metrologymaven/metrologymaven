---
id: IO.CAD.EXPORT.IGESW
title: IGES export of geometric elements
level: 4
parent: IO.CAD.EXPORT
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.CAD.EXPORT-001, REQ-IO.CAD.EXPORT-003, REQ-IO.CAD.EXPORT-004, REQ-IO.CAD.EXPORT-005]
covers: [GOM.EXP.07, PW.EXP.03]
depends_on: [IO.CAD.EXPORT.BOUND, IO.CAD.IGES.SECT, IO.CAD.IGES.ENT, IO.REG.ENV, IO.REG.WRITE]
side_effect: W1
determinism: D0
cli: [fid export cad-iges]
mcp: [export_cad_iges]
journal: [export.cad_iges@1]
updated: 2026-11-17
---

# IO.CAD.EXPORT.IGESW: IGES export of geometric elements

## Purpose

`export.cad_iges@1` writes selected geometric elements as IGES entities in the caller's frame and unit, with element names as entity labels, for CAD systems and incumbent workflows that hand features over as IGES. It is a first-party writer on the shared IGES section writer.

## Scope

**In:**
- Mapping: point → 116; bounded line → 110; circle and arc → 100 (with a 124 when not in the XY plane of the target frame); plane → 128 bilinear patch with a 144 trim; cylinder, cone, sphere → 128 exact rational B-spline surfaces (default) or 190–196 analytic surfaces when `analytic=true` [unverified: importer support for 190–196]; NURBS curves and surfaces → 126 and 128
- Global section: unit flag and name for `mm`, `m` or `inch`, model-space scale 1, minimum resolution, originating system `fid` and version
- Labels: element names in the 8-character DE label field (truncated, with the full name in a 406 name property [unverified: form number]) 
- `export.cad_iges@1` (W1, `fs_effect: write_root`)

**Out:**
- Extents and frame transforms (`IO.CAD.EXPORT.BOUND`)
- IGES solids (186), annotation

## Requirements

- **REQ-IO.CAD.EXPORT.IGESW-001** The exporter shall write the selected elements in the target frame and chosen unit, with the unit flag and unit name set in the Global section and recorded in the export report.
- **REQ-IO.CAD.EXPORT.IGESW-002** If a selected element type has no IGES mapping, then the export shall fail with `element_unsupported` naming it, unless `skip_unsupported` is set, in which case it shall be listed in the export report.
- **REQ-IO.CAD.EXPORT.IGESW-003** When the written file is re-imported with the IGES section parser and entity mapper, each element shall match the planned element within 1e-9 m and 1e-9 rad, compared by defining parameters for 116, 110 and 100 entities and by sampled surface points (at least 25 × 25 per patch) for B-spline encodings [unverified: tolerance].
- **REQ-IO.CAD.EXPORT.IGESW-004** Each element's name shall be written so that re-import recovers it exactly, using the label field when it fits in 8 characters and a name property otherwise.
- **REQ-IO.CAD.EXPORT.IGESW-005** Exporting the same elements with the same options twice shall produce byte-identical files, with the Global timestamp taken from the command's journal timestamp.

## Acceptance criteria

- **AC-1** Given `fixtures/io/cad/export/stepw/features.json`, when `fid export cad-iges --elements ... --unit mm --target <handle> --json` runs, then the file parses with `IO.CAD.IGES.SECT`, holds 116, 110, 100, 128/144 and 128 entities, and the Global unit flag is millimetre.
- **AC-2** Given the AC-1 file re-imported with the IGES entity mapper, then point, line and circle parameters match within 1e-9 m and the cylinder surface matches the planned cylinder at 25 × 25 samples within 1e-9 m.
- **AC-3** Given a feature named `DATUM_PLANE_A12`, then the re-imported name equals it exactly.
- **AC-4** (error path) Given a selection including a point cloud, then `element_unsupported`; with `--skip-unsupported` it is listed.
- **AC-5** Given the same export twice, then the files are byte-identical.

## Design

- **Registry commands:** `export.cad_iges@1` (W1, `fs_effect: write_root`; `elements`, `target`, `frame?`, `unit: mm|m|inch`, `extents?`, `margin?`, `analytic?: bool`, `skip_unsupported?`, `overwrite?`, `dry_run?`; result `ExportReport`). CLI `fid export cad-iges`, MCP `export_cad_iges`.
- **Algorithm:** plan → map to entities in plan order → exact rational quadratic NURBS for cylinders, cones and spheres (`fid-num` for trigonometric weights) → section writer → atomic write.
- **Errors:** `element_unsupported`, `extent_required`, `target_exists`, `raw_path_forbidden`, `path_outside_root`.
- **Determinism:** D0.
- **Performance budget:** 10^4 elements in < 1 s [unverified until measured].
- **Crate:** `fid-io-cad-export`.

## Test fixtures

Shares `fixtures/io/cad/export/stepw/features.json`; extra fixtures under `fixtures/io/cad/export/igesw/` (CC-BY-4.0).

## Build prompt

> In crate `fid-io-cad-export`, implement `export.cad_iges@1` (W1, `fs_effect: write_root`; CLI `fid export cad-iges`; MCP `export_cad_iges`) on the `fid-io-reg` export skeleton. Plan with the bound module; map points (116), bounded lines (110), circles/arcs (100 + 124 when off-plane), planes (128 + 144), cylinders/cones/spheres (exact rational 128 by default, 190–196 when `analytic=true`), NURBS (126/128); set the Global unit flag/name for mm/m/inch and timestamp from the journal; names in the DE label or a name property when longer than 8 characters; write with the `fid-io-iges` section writer; atomic write. Unmapped → `element_unsupported` unless `skip_unsupported`. Round-trip with the `fid-io-iges` parser and mapper within 1e-9 m. KNP-1 arithmetic, `fid-num` trig. CONSTITUTION §8: no panic, no unwrap. Tests AC-1..AC-5.

## Open questions

- REQ-IO.CAD.EXPORT-004 asks for parameter equality; for primitives encoded as B-splines this leaf checks sampled geometry instead, since parameter recovery from NURBS would need recognition.

## Children

_Leaf: none._
