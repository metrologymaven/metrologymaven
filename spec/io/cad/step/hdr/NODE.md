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
built_in: [fid-io-step]
satisfies: [REQ-IO.CAD.STEP-003, REQ-IO.CAD.STEP-006]
covers: []
depends_on: [IO.CAD.STEP.P21, IO.REG.PROV]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-20
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

### Pinned at build (fid-io-step module `header`)

These pins were made when the leaf was built. Where one differs from the text above, the pin wins, and the reason is given.

- **API.** `resolve_header(&P21File, &HeaderOptions) -> Result<StepHeaderInfo, HeaderError>`. `HeaderOptions` holds the caller's `unit` (`LengthUnit` of `IO.REG.PROV`), `override_unit`, `max_contexts` (100,000) and `max_header_text_bytes` (64 KiB, all header strings together). Further fixed bounds: 64 units and 64 uncertainties per context, 256 items per header string list. Exceeding any bound is `limit_exceeded`.
- **Schema table.** Prefixes `AP242_MANAGED_MODEL_BASED_3D_ENGINEERING` (ap242), `AUTOMOTIVE_DESIGN` (ap214), `CONFIG_CONTROL_DESIGN` and `AP203_CONFIGURATION_CONTROLLED_3D_DESIGN_OF_MECHANICAL_PARTS_AND_ASSEMBLIES` (ap203) [unverified against CAx-IF files]. The identifier is trimmed of ASCII whitespace and ASCII upper-cased (no Unicode case folding). After the prefix, the next character must be the end, a space, `{` or `_`, so `AUTOMOTIVE_DESIGNER` is refused. `FILE_SCHEMA` must name exactly one identifier (`malformed`, `file_schema_not_single`, otherwise). `FILE_SCHEMA`, `FILE_DESCRIPTION` and `FILE_NAME` must each appear once, with their full attribute count. `$` in a header string reads as empty.
- **Header text.** Kept as `StepText` (no `&str` view, no `Display`). It is shown only through `UntrustedText::as_display`. The `step_schema_unsupported` message puts it in double quotes, with `"` escaped. It is never interpreted.
- **Units.** Length: SI metre with no prefix or with centi, milli, micro or nano. Conversion-based `INCH` or `FOOT` (ASCII case-insensitive) over an SI length unit. Plane angle: SI radian, or conversion-based `DEGREE` over SI radian. Simple-instance and complex-instance forms of `SI_UNIT`, `CONVERSION_BASED_UNIT` and the measure-with-unit entities are read. References are followed at most two levels (unit, then measure, then SI base). A conversion-based base is `malformed` (`conversion_base_not_si`), which also stops reference cycles. An unknown conversion-based name or SI prefix is the added error **`step_unit_unsupported`**: not in the Design's list, but such a file is not malformed. Two length (or angle) units in one context, a unit that is both length and angle, a dangling unit reference, wrong attribute types and a non-positive conversion factor are `malformed`. Other units (solid angle and so on) are ignored.
- **Factor check.** The file factor is `value * base factor` in f64, compared with the exact factor at 1e-12 relative (`length_factor_mismatch`). For degree, the value is compared with pi/180 at 1e-6 relative (`angle_factor_mismatch`; not in the REQs, and looser because files commonly write 0.0174532925). The exact factor is always used. Warnings are deduplicated by unit entity, and `record_warnings` puts them in the import report.
- **Angle may be absent.** `UnitContext.angle` is `Option<AngleUnit>`, because files may declare none (deviation from `angle: AngleUnit`). `UnitContext` also carries `length_unit`, the file's `declared` unit and `owns_geometry`.
- **Uncertainty** is the first `global_uncertainty_assigned_context` entry whose unit is a length unit. It is `Uncertainty { value, authored, unit, length_factor_m }`, in the file's own unit and never converted (deviation from `Option<f64>`, so the unit is not lost). A negative value is `malformed`.
- **"Owns geometry"** means another instance refers to the context. A context with no length unit that owns no geometry, and gets no caller unit, is listed in `unitless`, not refused.
- **Caller unit.** Per context, the `IO.REG.PROV` table: a caller unit fills a context that declares none (`caller`); a different caller unit on a declared context is `unit_conflict` unless `override_unit` (`override`); `override_unit` without a unit is `unit_required`. Contexts are checked in ascending entity number, and the first refusal is returned.
- **Provenance (REQ-004).** `provenance_unit()` gives the `UnitResolution` for `SourceProvenance` from the primary context: the lowest-numbered context that owns geometry, else the lowest. With no context, it comes from the caller's unit alone. `IO.REG.PROV` has no centimetre, so a centimetre primary context is `step_unit_unsupported` there, though it resolves (0.01) in the table. `record_provenance()` declares `step.schema`, `step.schema_identifier`, `step.originating_system`, `step.file_name`, `step.time_stamp`, `step.author`, `step.organization` (JSON arrays), `step.preprocessor_version`, `step.authorization`, `step.description` (JSON array), `step.implementation_level`, `step.contexts` (the count), `step.unitless_contexts`, `step.units.mixed` and `step.units.group_<k>`. Contexts are grouped by their unit signature (unit, factor, source, angle, authored uncertainty, geometry) with a count and the first context, because `IO.REG.PROV` allows at most 256 declared items and assemblies can have more contexts than that. The full per-context table is reproducible from the file (D0), whose digest the record holds. Nothing is truncated: a value over `IO.REG.PROV`'s 4,096-byte limit fails as it does there.
- **Fixtures.** `fixtures/io/cad/step/hdr/make_cases.py` (stdlib only) writes 19 synthetic `.stp` files and `expected.txt`, with 25 cases worked out by its own reader and resolver. CI runs it with `--check`. The tests add bound cases, structural surprises, deterministic byte-mutation fuzzing of every fixture and random unit graphs. A cargo-fuzz target `step_header` sits next to `p21_parse`. No third-party STEP files and no standards text.

## Test fixtures

Hand-written header fixtures under `fixtures/io/cad/step/hdr/` (CC-BY-4.0).

## Build prompt

> In crate `fid-io-step`, add header and unit resolution over a parsed `P21File`: accept schemas AP242, AP214, AP203 (fixed identifier table; else `step_schema_unsupported` with the raw identifier as untrusted text); resolve each geometric representation context's length unit (SI with prefix, or conversion-based inch/foot with exact factors 0.0254/0.3048; warn if the file's factor differs by > 1e-12 relative), plane-angle unit and declared uncertainty; no length unit and no caller `unit` → `unit_required`; record schema, units, uncertainty and originating system in provenance via `IO.REG.PROV`. No standards text. CONSTITUTION §8: no panic, no unwrap; BTreeMap only. Tests AC-1..AC-4.

## Open questions

- Final accepted schema list and identifiers [unverified; carried from IO.CAD]. AP209 and AP238 are deliberately not accepted.
- `GEO.FRAME.UNITS` has no centimetre, so a centimetre file cannot yet carry its unit in the provenance record (it fails `step_unit_unsupported` there). Should `LengthUnit` gain `cm`?
- Should a multi-identifier `FILE_SCHEMA` be accepted when every identifier maps to the same schema? It is refused today, to fail closed.

## Children

_Leaf: none._
