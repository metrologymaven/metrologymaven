---
id: GDT.DRF.DATUM
title: Datum feature and datum target definitions
level: 3
parent: GDT.DRF
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.DRF-001]
covers: [GOM.GDT.14, SA.GDT.03]
depends_on: [GDT.CHAR.MODEL]
side_effect: W2
determinism: D0
cli: [fid gdt define-datum]
mcp: [gdt_define_datum]
journal: [gdt.define_datum@1]
updated: 2026-11-16
---

# GDT.DRF.DATUM: Datum feature and datum target definitions

## Purpose

Lets a user (or an adopted PMI proposal) declare a datum: its label, the datum feature it is bound to (a measured feature and its nominal CAD faces), the feature type, and optional datum targets. A DRF can only be built from declared datums, so this is the first step of every datum-related check. The definition is data in the characteristic model; this leaf owns the command and its validation.

## Scope

**In:** `gdt.define_datum@1`; `DatumDefinition { label, feature_ref, nominal_faces, feature_type, targets }`; datum target references (point, line, area) as data; uniqueness of labels per plan; redefinition as a new journaled event.
**Out:** building DRFs (owned by `GDT.DRF.BUILD`); simulating datum targets and common datums (owned by `GDT.DRF.COMMON`); the characteristic and FCF types (owned by `GDT.CHAR.MODEL`).

## Requirements

- **REQ-GDT.DRF.DATUM-001** When `gdt.define_datum@1` succeeds, the system shall store a datum definition with label, feature reference, nominal face IDs, feature type and optional targets, and journal one event with the full definition.
- **REQ-GDT.DRF.DATUM-002** If the label already exists in the plan, then the command shall fail with `datum_label_exists` unless `--replace` is given, in which case the event shall record the previous definition.
- **REQ-GDT.DRF.DATUM-003** If the feature type is not one of the types the DMT schema lists as datum-capable, then the command shall fail with `unsupported_datum_feature_type`.
- **REQ-GDT.DRF.DATUM-004** If the referenced feature or any nominal face ID does not exist, then the command shall fail with `reference_missing` naming the ID.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/drf/block_plan.fidproj` (a block with three measured planes), when `fid gdt define-datum --label A --feature f1 --faces face:12 --type plane --json` runs, then datum A exists and one event holds its definition.
- **AC-2** (error path) Given a second `--label A` without `--replace`, then `datum_label_exists` and nothing is journaled.
- **AC-3** Given `--replace`, then the event records old and new definitions.
- **AC-4** (error path) Given `--faces face:999`, then `reference_missing{face:999}`.
- **AC-5** (replay) Exact replay of AC-1 and AC-3 gives `IDENTICAL`.

## Design

- **Registry command(s):** `gdt.define_datum@1`, W2, `--json`, `--dry-run`. Params: `label: String` (1–3 characters, validated against a per-dialect pattern from the defaults profile), `feature: FeatureRef`, `faces: [FaceId]`, `type: DatumFeatureType`, `targets: [DatumTargetRef]`, `replace: bool`.
- **Data structures:** `DatumDefinition`, `DatumTargetRef { Point | Line | Area }` (geometry references only).
- **Algorithm:** validate references and type, check label uniqueness, write, journal.
- **Errors:** `datum_label_exists`, `unsupported_datum_feature_type`, `reference_missing{id}`, `bad_label`.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-gdt`.

## Test fixtures

`block_plan.fidproj`, a synthetic block project generated in-repo; CC0.

## Build prompt

> In crate `fid-gdt`, implement `gdt.define_datum@1` (W2; CLI `fid gdt define-datum`; MCP `gdt_define_datum`; `--json`, `--dry-run`; one journal event with the full definition). Store `DatumDefinition { label, feature_ref, nominal_faces, feature_type, targets }` in the plan's characteristic model. Validate: label pattern from the active defaults profile (`bad_label`), unique label unless `replace` (`datum_label_exists`; on replace record old and new), datum-capable feature type per the DMT schema (`unsupported_datum_feature_type`), referenced IDs exist (`reference_missing{id}`). Datum targets are stored as geometry references only. Tests: acceptance criteria above. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), no std `HashMap`/`HashSet`.

## Open questions

- None.

## Children

_Leaf: none._
