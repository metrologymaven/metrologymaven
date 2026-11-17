---
id: GDT.CHAR.MODEL.TYPES
title: Characteristic and FCF types with JSON Schemas
level: 4
parent: GDT.CHAR.MODEL
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.CHAR.MODEL-001, REQ-GDT.CHAR.MODEL-002, REQ-GDT.CHAR.MODEL-004]
covers: [PCD.DIM.02]
depends_on: [IO.QIF, GEO.FRAME.UNITS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.CHAR.MODEL.TYPES: Characteristic and FCF types with JSON Schemas

## Purpose

The Rust types every GDT child reads: `Characteristic` (definition, nominal, item, measurement), `CharacteristicKind` with one variant per QIF 3.0 characteristic type, `FeatureControlFrame`, `DatumReference`, `CompositeSegment`, `SimultaneousGroup`, plus an `Extension` variant that carries a characteristic owned by another domain (for example SCAN comparison points) as an opaque payload. Types are data only: no evaluation and no interpretation. Their JSON Schemas (2020-12) are the MCP output schemas.

## Scope

**In:** the types above; `CharacteristicKind` checked against the generated QIF 3.0 type list at build time; FCF fields (symbol, tolerance value, zone shape, material modifiers on the toleranced feature and on datum references, other QIF modifier enumerations, composite segments up to the QIF maximum, simultaneous-requirement group ID); authored values as `AuthoredQuantity` (exact decimal text plus unit); the `Extension { owner, kind, schema_version, payload }` variant; structural validation limited to what the QIF 3.0 XSD itself constrains; JSON Schema export.
**Out:** mapping to and from QIF model types (owned by `GDT.CHAR.MODEL.QIFMAP`); create and edit commands (owned by `GDT.CHAR.MODEL.CMDS`); governing spec (owned by `GDT.CHAR.SPEC`); identity (owned by `GDT.CHAR.IDENT`); the content and meaning of extension payloads (owned by their owner, for example `SCAN.CMP.CPT`); any rule about which modifier combinations are legal under a dialect (owned by DMT rows via `GDT.RES`).

## Requirements

- **REQ-GDT.CHAR.MODEL.TYPES-001** `CharacteristicKind` shall have exactly one variant per characteristic type in the pinned QIF 3.0 characteristic schema, and a build-time test shall fail when the generated QIF type list contains a type with no variant.
- **REQ-GDT.CHAR.MODEL.TYPES-002** `FeatureControlFrame` shall represent every QIF 3.0 modifier enumeration value, zone shape, datum-reference material boundary modifier and composite segment up to the QIF maximum segment count, verified by a test that enumerates the generated enumerations.
- **REQ-GDT.CHAR.MODEL.TYPES-003** Every authored tolerance value, limit and nominal size shall be stored as an `AuthoredQuantity` (exact decimal text and unit); the model shall hold no f64 for an authored value.
- **REQ-GDT.CHAR.MODEL.TYPES-004** If a constructor receives a structure the QIF 3.0 XSD forbids (for example more composite segments than the QIF maximum, or a datum reference with an empty label), then it shall return `invalid_fcf{reason}` and no value.
- **REQ-GDT.CHAR.MODEL.TYPES-005** The `Extension` variant shall carry an owner tag from a closed registry list, a kind string of at most 64 characters, a schema version and a canonical-JSON payload, and GDT code shall never read the payload.
- **REQ-GDT.CHAR.MODEL.TYPES-006** Each public type shall export a JSON Schema 2020-12 document, and the exported schema bytes shall be identical across platforms.

## Acceptance criteria

- **AC-1** Given the generated QIF 3.0 characteristic type list, when the build-time test runs, then every type maps to one `CharacteristicKind` variant; removing one variant makes the test fail.
- **AC-2** Given fixture `fixtures/gdt/model/fcf_all_modifiers.json` (one FCF per QIF modifier value and zone shape, a composite FCF at the QIF maximum segment count), when deserialised and re-serialised, then the output equals the input byte for byte (RFC 8785 canonical JSON).
- **AC-3** (error path) Given a composite FCF with one segment more than the QIF maximum, then construction fails with `invalid_fcf{reason: composite_segment_limit}`.
- **AC-4** Given tolerance text `0.050` in mm, then the stored value keeps the text `0.050` and unit `mm`, and no f64 field appears in the serialised characteristic.
- **AC-5** Given an `Extension` with owner `scan.cmp` and an arbitrary payload, then it round-trips through serde unchanged; given an owner not in the registry list, then `unknown_extension_owner`.
- **AC-6** Given the exported JSON Schemas, when validated against AC-2's fixture with a 2020-12 validator, then validation passes, and the schema bytes are identical on all CI platforms.

## Design

- **Registry command(s):** none (types).
- **Data structures:** `Characteristic { id: CharId, definition, nominal, item, measurement?, kind: CharacteristicKind, fcf: Option<FeatureControlFrame>, ext_attrs }`; `FeatureControlFrame { symbol, segments: Vec<CompositeSegment>, sim_group: Option<GroupId> }`; `CompositeSegment { tolerance: AuthoredQuantity, zone_shape, material: Option<MaterialModifier>, modifiers: BTreeSet<QifModifier>, datums: Vec<DatumReference> }`; `DatumReference { label, material_boundary: Option<..>, modifiers }`; `Extension { owner: ExtOwner, kind: String, schema_version: SemVer, payload: CanonicalJson }`.
- **Algorithm:** enum variants generated from or checked against `fid-qif-model` (IO.QIF.GEN output); serde with `deny_unknown_fields`; schema export with `schemars`-style derivation.
- **Errors:** `invalid_fcf{reason}`, `unknown_extension_owner`, `field_too_long`.
- **Determinism:** D0 (no arithmetic).
- **Performance budget:** not critical; deserialise 10 000 characteristics < 100 ms [unverified].
- **Crate:** `fid-gdt` (module `model`).

## Test fixtures

Hand-written synthetic FCF fixtures covering every QIF enumeration value (CC0, in-repo); the generated QIF type list from `fid-qif-model`.

## Build prompt

> In crate `fid-gdt`, module `model`, define the characteristic data types: `Characteristic`, `CharacteristicKind` (exactly one variant per QIF 3.0 characteristic type; add a build-time test against the generated `fid-qif-model` type list), `FeatureControlFrame`, `CompositeSegment` (up to the QIF maximum count), `DatumReference` (with material boundary modifiers), `SimultaneousGroup`, and `Extension { owner, kind (<= 64 chars), schema_version, payload: canonical JSON }` whose owner comes from a closed list (initially `scan.cmp`) and whose payload GDT never reads. Authored values use `AuthoredQuantity` from `fid-geo-frame` (exact decimal text plus unit; no f64). Constructors enforce only structural limits the QIF XSD imposes and return `invalid_fcf{reason}`. Export JSON Schema 2020-12 per public type. Tests per acceptance criteria (canonical JSON round-trip of `fixtures/gdt/model/fcf_all_modifiers.json`, error paths, schema validation, identical schema bytes across platforms). Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`; all input hostile), no std `HashMap`/`HashSet`; no dialect rules in this crate module (those are DMT data).

## Open questions

- The QIF maximum composite segment count must be read from the pinned XSD, not assumed.

## Children

_Leaf: none._
