---
id: IO.CAD.STEP.HDR
title: STEP schema detection and unit contexts
level: 4
parent: IO.CAD.STEP
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-IO.CAD.STEP-003, REQ-IO.CAD.STEP-006]
covers: []
depends_on: [IO.CAD.STEP.P21, IO.REG.PROV]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.CAD.STEP.HDR: STEP schema detection and unit contexts

## Purpose

Reads the STEP header to decide whether the file's schema is accepted, and resolves each geometric representation context's length unit, plane-angle unit and declared model uncertainty, so the geometry readers convert to metres with one exact factor per context and the provenance records what the file declared.

## Scope

**In:**
- `FILE_SCHEMA` matching against the accepted list: AP242 (managed model based 3D engineering), AP214 (automotive design) and AP203 (configuration controlled design, both editions) [unverified: exact schema identifiers to confirm against CAx-IF files]
- `FILE_NAME` and `FILE_DESCRIPTION` captured as untrusted metadata (originating system, author, timestamp)
- Unit resolution per `geometric_representation_context` with `global_unit_assigned_context`: SI length units with prefixes (milli, centi, none, micro), conversion-based length units (inch, foot) with their conversion factor, plane-angle units (radian, degree)
- `global_uncertainty_assigned_context` value recorded per context
- `UnitContext` table offered to `IO.CAD.STEP.GEOM` and `IO.CAD.PMI`

**Out:**
- Generic unit and provenance rules (`IO.REG.PROV`)
- Parsing (`IO.CAD.STEP.P21`)

## Requirements

- **REQ-IO.CAD.STEP.HDR-001** If `FILE_SCHEMA` names no schema in the accepted list, then the import shall fail with `step_schema_unsupported` naming the schema identifier as untrusted text.
- **REQ-IO.CAD.STEP.HDR-002** For each representation context, the system shall resolve the length unit to an exact factor to metres, and if a conversion-based unit's factor disagrees with the known exact factor by more than 1e-12 relative, then the import report shall warn and the exact factor shall be used.
- **REQ-IO.CAD.STEP.HDR-003** If a representation context that owns geometry declares no length unit, then the import shall fail with `unit_required` unless the caller supplied `unit`, which shall be recorded as an override in the provenance.
- **REQ-IO.CAD.STEP.HDR-004** The declared schema, every context's unit and uncertainty, and the originating-system string shall be recorded in the import provenance.

## Acceptance criteria

- **AC-1** Given `fixtures/io/cad/step/hdr/ap242_mm.stp`, when headers are resolved, then schema `ap242`, one context with factor 0.001 and uncertainty 1e-6 (file units) are returned.
- **AC-2** Given `fixtures/io/cad/step/hdr/ap214_inch.stp` (conversion-based inch with factor 25.4 mm), then the factor is exactly 0.0254.
- **AC-3** (error path) Given `fixtures/io/cad/step/hdr/ap209.stp`, then resolution fails `step_schema_unsupported`.
- **AC-4** (error path) Given `fixtures/io/cad/step/hdr/no_unit.stp` without `unit`, then `unit_required`; with `unit=mm`, success and `unit_overridden: true` in provenance.

## Design

- **Registry commands:** none (library).
- **Data structures:** `StepSchema { Ap242, Ap214, Ap203 }`; `UnitContext { context: u64, length_factor_m: f64, angle: AngleUnit, uncertainty: Option<f64>, source: UnitSource }`; `StepHeaderInfo { schema, raw_schema: Untrusted, originating_system: Untrusted, contexts: BTreeMap<u64, UnitContext> }`.
- **Algorithm:** match `FILE_SCHEMA` identifiers case-insensitively by prefix against a fixed table; walk context complex instances; known exact factors table for inch (0.0254) and foot (0.3048).
- **Errors:** `step_schema_unsupported`, `unit_required`, `malformed`.
- **Determinism:** D0.
- **Performance budget:** negligible against parsing.
- **Crate:** `fid-io-step`.

## Test fixtures

Hand-written header fixtures under `fixtures/io/cad/step/hdr/` (CC-BY-4.0).

## Build prompt

> In crate `fid-io-step`, add header and unit resolution over a parsed `P21File`: accept schemas AP242, AP214, AP203 (fixed identifier table; else `step_schema_unsupported` with the raw identifier as untrusted text); resolve each geometric representation context's length unit (SI with prefix, or conversion-based inch/foot with exact factors 0.0254/0.3048; warn if the file's factor differs by > 1e-12 relative), plane-angle unit and declared uncertainty; no length unit and no caller `unit` → `unit_required`; record schema, units, uncertainty and originating system in provenance via `IO.REG.PROV`. No standards text. CONSTITUTION §8: no panic, no unwrap; BTreeMap only. Tests AC-1..AC-4.

## Open questions

- Final accepted schema list and identifiers [unverified; carried from IO.CAD]. AP209 and AP238 are deliberately not accepted.

## Children

_Leaf: none._
