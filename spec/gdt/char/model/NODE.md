---
id: GDT.CHAR.MODEL
title: Characteristic and feature-control-frame model on QIF 3.0
level: 3
parent: GDT.CHAR
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.CHAR-005]
covers: [PCD.DIM.02]
depends_on: [IO.QIF, CORE.PRJ]
cli: [fid gdt create-characteristic, fid gdt edit-characteristic]
mcp: [gdt_create_characteristic, gdt_edit_characteristic]
journal: [gdt.create_characteristic@1, gdt.edit_characteristic@1]
updated: 2026-11-16
---

# GDT.CHAR.MODEL: Characteristic and feature-control-frame model on QIF 3.0

## Purpose

Defines the in-memory characteristic types every GDT child reads: QIF 3.0 (ISO 23952) characteristic definition, nominal, item and measurement, and the feature control frame (FCF) with its symbol, tolerance value, zone shape, modifiers, datum references, composite segments and simultaneous-requirement groups. It holds these as data only; it never evaluates them. It is separate from the governing spec, defaults profiles and ResultRecord because those carry policy, while this module is a faithful, lossless mirror of the QIF characteristic schema.

## Scope

**In:**
- Rust types mirroring QIF 3.0 characteristic definition / nominal / item / measurement for every characteristic type QIF 3.0 defines (dimensional and geometric)
- FCF elements: symbol, tolerance value and unit, zone shape (diameter, spherical diameter, width), material modifiers on the toleranced feature and on datum references, other modifiers (projected zone, free state, tangent plane, statistical, unequally disposed and similar QIF enumerations), composite segments up to the QIF maximum, simultaneous-requirement groups
- Datum reference and datum-feature reference slots (the definitions are created by `GDT.DRF.DATUM`)
- Fiducial extensions only through QIF `Attributes` hooks
- Commands to create and edit a characteristic (W2), with JSON Schema 2020-12 output schemas for MCP
- Lossless round-trip of the above through QIF import and export (IO parses and writes; this module maps)

**Out** (and which node owns it instead):
- Governing spec and its validation (owned by `GDT.CHAR.SPEC`)
- Defaults profiles (owned by `GDT.CHAR.DPROF`)
- Characteristic identity rules (owned by `GDT.CHAR.IDENT`)
- ResultRecord (owned by `GDT.CHAR.RECORD`)
- QIF XML parsing and writing (owned by `IO.QIF`)
- Datum definition commands (owned by `GDT.DRF.DATUM`)

## Requirements

- **REQ-GDT.CHAR.MODEL-001** The characteristic model shall have one variant for every characteristic type defined in the QIF 3.0 characteristic schema, and a QIF characteristic type with no variant shall fail import with a typed `unsupported_characteristic_type` error rather than be dropped.
- **REQ-GDT.CHAR.MODEL-002** The FCF model shall represent every modifier, zone shape, datum-reference modifier and composite segment that QIF 3.0 can encode, up to the QIF maximum number of composite segments.
- **REQ-GDT.CHAR.MODEL-003** When a QIF document from the IO round-trip corpus is imported and exported, the exported characteristic elements shall be semantically equal to the imported ones (same element set, values, enumerations and IDs) for 100 % of corpus files.
- **REQ-GDT.CHAR.MODEL-004** Tolerance values shall be stored as exact authored decimals with unit; conversion to SI f64 shall happen only through the single journaled conversion of CONSTITUTION §4.
- **REQ-GDT.CHAR.MODEL-005** When a characteristic is created or edited, the system shall journal one `gdt.create_characteristic@1` or `gdt.edit_characteristic@1` event whose payload is the full post-change characteristic in canonical JSON.

## Interfaces

- **Offers:** `Characteristic`, `FeatureControlFrame`, `DatumReference`, `CompositeSegment`, `SimultaneousGroup` types and their JSON Schemas; `gdt.create_characteristic@1` (W2), `gdt.edit_characteristic@1` (W2).
- **Consumes:** QIF types and the round-trip corpus (IO.QIF, IO.CORP); project container (CORE.PRJ).

## Decomposition hints

TYPES (Rust types and JSON Schema), QIFMAP (mapping to and from IO.QIF types), CMDS (create and edit commands), RTRIP (round-trip corpus test).

## Open questions

- Which QIF 3.0 characteristic types are needed in P1a versus later; the full set is large. A P1a subset may need the parent to accept a staged REQ-GDT.CHAR-005.

## Children

_None yet: status `open`._
