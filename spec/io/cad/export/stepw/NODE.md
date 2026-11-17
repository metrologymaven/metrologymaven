---
id: IO.CAD.EXPORT.STEPW
title: STEP export of geometric elements
level: 4
parent: IO.CAD.EXPORT
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.CAD.EXPORT-001, REQ-IO.CAD.EXPORT-003, REQ-IO.CAD.EXPORT-004, REQ-IO.CAD.EXPORT-005]
covers: [GOM.EXP.07]
depends_on: [IO.CAD.EXPORT.BOUND, IO.CAD.STEP.P21, IO.CAD.STEP.GEOM, IO.REG.ENV, IO.REG.WRITE]
side_effect: W1
determinism: D0
cli: [fid export cad-step]
mcp: [export_cad_step]
journal: [export.cad_step@1]
updated: 2026-11-17
---

# IO.CAD.EXPORT.STEPW: STEP export of geometric elements

## Purpose

`export.cad_step@1` writes selected geometric elements, nominal or measured, as a geometry-only STEP AP242 file (wireframe and surface geometry in a geometric set) in the caller's frame and length unit, with element names on the entities, so the geometry can go back into a CAD system. It is a first-party writer on the shared Part 21 printer; no CAD kernel is involved.

## Scope

**In:**
- Mapping: point → `cartesian_point`; line → bounded `trimmed_curve` on a `line`; circle and arc → `circle` (trimmed for arcs); plane → `rectangular_trimmed_surface` on a `plane`; cylinder, cone, sphere → their analytic surfaces (trimmed where bounded); NURBS curves and surfaces → `b_spline_*_with_knots` (rational complex form when weighted) [unverified: schema choice of the containing representation]
- Header with schema AP242, a length unit (`mm`, `m` or `inch`) and plane-angle unit radian, originating system `fid` and version
- Names as `name` attributes (untrusted text, escaped)
- `skip_unsupported` handling and the export report
- `export.cad_step@1` (W1, `fs_effect: write_root`)

**Out:**
- Extents and frame transforms (`IO.CAD.EXPORT.BOUND`)
- PMI and B-rep solid export (not required by IO.CAD)

## Requirements

- **REQ-IO.CAD.EXPORT.STEPW-001** The exporter shall write the selected elements in the target frame and the chosen length unit, and record frame and unit in the file's unit context and in the export report.
- **REQ-IO.CAD.EXPORT.STEPW-002** If a selected element type has no STEP mapping, then the export shall fail with `element_unsupported` naming it, unless `skip_unsupported` is set, in which case it shall be listed in the export report and the rest written.
- **REQ-IO.CAD.EXPORT.STEPW-003** When the written file is read back with the STEP Part 21 parser and geometry mapper, each element's defining parameters shall match the planned element within 1e-9 m and 1e-9 rad [unverified: tolerance].
- **REQ-IO.CAD.EXPORT.STEPW-004** Each element's name shall be written as the entity's `name` attribute, escaped so that it parses back to the same string.
- **REQ-IO.CAD.EXPORT.STEPW-005** Exporting the same elements with the same options twice shall produce byte-identical files, apart from the header timestamp, which shall be taken from the command's journal timestamp.

## Acceptance criteria

- **AC-1** Given `fixtures/io/cad/export/stepw/features.json` (a point, a bounded line, a circle, a plane with extent, a cylinder with axial extent, all named), when `fid export cad-step --elements ... --unit mm --frame world --target <handle> --json` runs, then the file parses, holds 5 named geometric items in a unit context of millimetre, and the report lists unit, frame and extents.
- **AC-2** Given the file of AC-1 read back with the STEP parser and geometry mapper, then each defining parameter matches within 1e-9 m / 1e-9 rad.
- **AC-3** (error path) Given a selection including a mesh, then `element_unsupported{element}` and no file is written; with `--skip-unsupported`, the file is written and the mesh is listed.
- **AC-4** Given a name containing `'` and non-ASCII characters, then it round-trips exactly.
- **AC-5** Given the same export twice, then the files are byte-identical.

## Design

- **Registry commands:** `export.cad_step@1` (W1, `fs_effect: write_root`; `elements: [ElementId]`, `target: RootHandle`, `frame?: FrameId` (default world), `unit: mm|m|inch`, `extents?: {ElementId: Extent}`, `margin?`, `skip_unsupported?`, `overwrite?`, `dry_run?`; result `ExportReport`). CLI `fid export cad-step`, MCP `export_cad_step`.
- **Algorithm:** plan via `IO.CAD.EXPORT.BOUND` → map each item to Part 21 instances with ascending entity numbers in plan order → convert metres to output unit with one division → print with the shared printer (shortest round-trip reals) → `IO.REG.WRITE` atomic write.
- **Errors:** `element_unsupported`, `extent_required`, `target_exists`, `raw_path_forbidden`, `path_outside_root`.
- **Determinism:** D0.
- **Performance budget:** 10^4 elements in < 1 s [unverified until measured].
- **Crate:** `fid-io-cad-export`.

## Test fixtures

JSON element fixtures under `fixtures/io/cad/export/stepw/` (CC-BY-4.0). Compatibility with commercial CAD importers is checked manually and recorded [unverified].

## Build prompt

> In crate `fid-io-cad-export`, implement `export.cad_step@1` (W1, `fs_effect: write_root`; CLI `fid export cad-step`; MCP `export_cad_step`) on the `fid-io-reg` export skeleton. Build the plan with the bound module, map points, bounded lines, circles/arcs, bounded planes, cylinders, cones, spheres and NURBS to AP242 geometric entities in a geometric set, with names as escaped `name` attributes; write header (AP242, chosen length unit mm/m/inch, radian, originating system `fid` + version, timestamp from the journal) with the `fid-io-step` Part 21 printer; one division per coordinate for unit conversion; atomic write via `IO.REG.WRITE`. Unmapped type → `element_unsupported` unless `skip_unsupported` (list it). Round-trip test with the `fid-io-step` parser and geometry mapper within 1e-9 m / 1e-9 rad; byte-identical output across runs. CONSTITUTION §8: no panic, no unwrap. Tests AC-1..AC-5.

## Open questions

- Which AP242 representation type to wrap free geometry in for best acceptance by CAD importers [unverified]; to confirm on CAx-IF practice.

## Children

_Leaf: none._
