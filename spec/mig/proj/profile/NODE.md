---
id: MIG.PROJ.PROFILE
title: Mapping profile schema and loader
level: 3
parent: MIG.PROJ
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-MIG.PROJ-001, REQ-MIG.PROJ-006]
covers: []
depends_on: [MIG.SRC.REG, IO.REG.DETECT, CORE.JRN.CANON]
uses_artifacts: [MIG.COVER.TAXO]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# MIG.PROJ.PROFILE: Mapping profile schema and loader

## Purpose

A mapping profile is data that tells the importers how one admitted incumbent export maps onto our model: which field or element holds the incumbent identifier, the feature label, coordinates, normals, unit, frame, alignment matrix, tolerance limits and datum references, and which taxonomy construct each source record type is. Profiles let a new documented export be supported by adding data rather than code, and each profile is bound to exactly one `MIG.SRC` register entry so no profile can open a route the register has not admitted.

## Scope

**In:**
- The `mig-profile@1` schema: profile ID and version, source register entry ID, format ID, incumbent namespace (string used to scope external IDs), field mappings per record type, construct ID per record type, unit and frame rules (declared in file, fixed by profile, or required from caller)
- Built-in generic profiles for open formats: `qif-generic`, `dmis-output-generic`, `csv-points-generic` (column map)
- Loader and validator with a canonical digest
- `cargo xtask mig profile-check`: schema, register binding and construct closure against taxonomies

**Out:**
- Applying profiles (owned by `MIG.PROJ.POINTS`, `MIG.PROJ.CHAR`, `MIG.PROJ.ALIGN`, `MIG.PROJ.PROJECT`)
- Register entries and admission (owned by `MIG.SRC`)

## Requirements

- **REQ-MIG.PROJ.PROFILE-001** Each profile shall name exactly one source register entry ID, one format ID, an incumbent namespace and a construct ID for every record type it maps.
- **REQ-MIG.PROJ.PROFILE-002** If a profile's register entry is not `ADMITTED` or `OPEN_EXPORT_ONLY`, then the loader shall refuse it with `profile_source_not_admitted`.
- **REQ-MIG.PROJ.PROFILE-003** If a profile names a construct ID absent from the format's taxonomy, then the profile check shall fail with `PROFILE-CONSTRUCT`.
- **REQ-MIG.PROJ.PROFILE-004** A profile shall declare, per record type, whether the unit is read from the file, fixed by the profile, or required from the caller, and shall never default a unit silently.
- **REQ-MIG.PROJ.PROFILE-005** The loader shall expose the profile version and the SHA-256 of its RFC 8785 canonical JSON, and importers shall journal both.

## Acceptance criteria

- **AC-1** Given `fixtures/mig/proj/profile/csv-points-generic.json` and a register extract where its source is `OPEN_EXPORT_ONLY`, when loaded, then loading succeeds and the digest equals `csv-points-generic.digest`.
- **AC-2** (error path) Given `pending.json` bound to a `PENDING_REVIEW` entry, then loading fails with `profile_source_not_admitted`.
- **AC-3** (error path) Given `bad-construct.json` naming `point_set.weighted`, when `cargo xtask mig profile-check` runs against the taxonomy fixtures, then it fails with `PROFILE-CONSTRUCT`.
- **AC-4** (error path) Given `no-unit-rule.json` (a record type without a unit rule), then loading fails with `profile_schema`.

## Design

- **Registry command(s):** none; library plus repository tool `cargo xtask mig profile-check --profiles <dir> --taxonomies <dir> --sources <json>`.
- **Data structures:** `MappingProfile { schema: "mig-profile@1", id, version, source_entry, format_id, namespace, record_types: Vec<RecordMapping> }`, `RecordMapping { record_type, construct_id, fields: BTreeMap<TargetField, SourceSelector>, unit: FromFile | Fixed(Unit) | Caller, frame: FromFile | Fixed(FrameRef) | Caller }`. `SourceSelector` is a column name or index for CSV, an element path for QIF, a statement field for DMIS output.
- **Algorithm:** bounded parse (256 KiB) → schema → register lookup → digest.
- **Errors:** `profile_schema`, `profile_source_not_admitted`, `PROFILE-CONSTRUCT`.
- **Determinism:** D0.
- **Performance budget:** under 5 ms per profile [unverified until measured].
- **Crate:** `fid-mig` (module `proj::profile`).

## Test fixtures

`fixtures/mig/proj/profile/`: generic profiles and broken variants written for the project, CC0-1.0.

## Build prompt

> In crate `fid-mig`, add module `proj::profile` implementing spec node MIG.PROJ.PROFILE: the `mig-profile@1` types, a bounded loader (256 KiB) that refuses profiles whose source register entry is not `ADMITTED`/`OPEN_EXPORT_ONLY` (`profile_source_not_admitted`) and profiles missing a unit or frame rule per record type (`profile_schema`), exposing version and RFC 8785 canonical-JSON SHA-256; the built-in generic profiles `qif-generic`, `dmis-output-generic`, `csv-points-generic`; and `cargo xtask mig profile-check` failing `PROFILE-CONSTRUCT` for construct IDs missing from the format taxonomy. Never default a unit silently. Tests for AC-1..AC-4. No `unwrap`/`expect`/`panic`/`unsafe`; BTreeMap only.

## Open questions

- None.

## Children

_Leaf: none._
