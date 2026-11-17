---
id: IO.QIF.IDS.EXT
title: Extension-hook pass-through and fx typed carriage
level: 4
parent: IO.QIF.IDS
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.QIF.IDS-004, REQ-IO.QIF.IDS-005]
covers: []
depends_on: [IO.QIF.GEN.RUST, IO.REG.LIMITS]
uses_artifacts: [CORE.PRJ.QIF]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.QIF.IDS.EXT: Extension-hook pass-through and fx typed carriage

## Purpose

Carries everything found inside QIF's sanctioned extension hooks (`Attributes`, `AttributeUser`, `UserDataXML` and the like) through import, edit and export: content the platform does not model is kept as a byte-faithful fragment, and `fx` elements defined by the CORE extension schema are parsed into typed values. When an edit orphans extension content, the export report says so instead of dropping it silently.

## Scope

**In:**
- Rules for the `ExtensionBag` container (defined in `fid-qif-ir`): opaque fragments with their in-scope namespace declarations, owned by one host entity
- Typed carriage of `fx` elements per the CORE extension schema version; unknown `fx` versions kept opaque with a warning
- Orphan and dangling-reference detection at export
- A size cap per bag under `IO.REG.LIMITS`

**Out:**
- The `fx` catalogue contents and extension schema (`CORE.PRJ.QIF`, read as an artifact)
- XML and JSON serialisation of bags (`IO.QIF.XML.WRITE`, `IO.QIF.JSON.PROJ`)
- Integer ID and QPId mapping (`IO.QIF.IDS.MAP`)

## Requirements

- **REQ-IO.QIF.IDS.EXT-001** Content inside a sanctioned extension hook that the system does not model shall be stored with its namespace context and re-emitted so that input and output are equal under `qif-norm@1`.
- **REQ-IO.QIF.IDS.EXT-002** Elements in the `fx` namespace whose version the CORE extension schema defines shall be parsed into typed values and re-emitted from those values.
- **REQ-IO.QIF.IDS.EXT-003** If an `fx` element names a version the extension schema does not define, then it shall be kept as opaque content and the import report shall carry warning `fx_version_unknown`.
- **REQ-IO.QIF.IDS.EXT-004** If an edit deletes the host entity of an extension bag, then the export report shall list the bag as `extension_dropped_with_host` with the host's QPId or `id`.
- **REQ-IO.QIF.IDS.EXT-005** If opaque or typed extension content contains a QPId of an entity deleted since import, then the export shall keep the content and the export report shall list `dangling_extension_ref` with the QPId and the bag's host.
- **REQ-IO.QIF.IDS.EXT-006** If one bag exceeds 16 MiB [unverified: tune on corpus], then the import shall fail with `limit_exceeded` naming the extension limit.

## Acceptance criteria

- **AC-1** Given `fixtures/io/qif/ids/ext/userdata.qif` with an unknown vendor element in `UserDataXML`, when imported and exported, then the output equals the input under `qif-norm@1`.
- **AC-2** Given `fixtures/io/qif/ids/ext/fx_v1.qif` with `fx:ResultProvenance` of a defined version, then the bag holds a typed value and re-export is equal under `qif-norm@1`.
- **AC-3** Given `fixtures/io/qif/ids/ext/fx_v99.qif`, then the content is kept opaque and the report has `fx_version_unknown`.
- **AC-4** Given AC-1's document and a deleted host feature, when exported, then the report lists `extension_dropped_with_host` with that feature's id.
- **AC-5** Given `fixtures/io/qif/ids/ext/qpid_ref.qif` where an `Attributes` value holds another entity's QPId, and that entity is deleted, then export keeps the attribute and reports `dangling_extension_ref`.
- **AC-6** (error path) Given a generated 17 MiB `UserDataXML` block, then import fails `limit_exceeded`.

## Design

- **Registry commands:** none (library).
- **Data structures:** `ExtensionBag { host: EntityId, items: Vec<ExtItem> }`; `ExtItem::Opaque { ns_context: Vec<(prefix, uri)>, bytes: Vec<u8> }` or `ExtItem::Fx { version, value: FxValue }`; `ExtReport { dropped: Vec<..>, dangling: Vec<..>, unknown_fx: Vec<..> }`.
- **Algorithm:** at import, capture the raw byte span of each hook element with its namespace context; parse `fx` items by a table generated from the CORE extension schema; at export, scan opaque bytes for UUID text and compare with the set of deleted QPIds (integer `id` references inside opaque content cannot be recognised and are documented as a limit).
- **Errors:** `limit_exceeded`; warnings `fx_version_unknown`, `extension_dropped_with_host`, `dangling_extension_ref`.
- **Determinism:** D0.
- **Performance budget:** pass-through adds < 5% to QIF import time [unverified until measured].
- **Crate:** `fid-qif` (module `ids::ext`).

## Test fixtures

Hand-written QIF documents under `fixtures/io/qif/ids/ext/` (CC-BY-4.0); the CORE `fx` extension schema from `CORE.PRJ.QIF`.

## Build prompt

> In crate `fid-qif`, add module `ids::ext` implementing `ExtensionBag` rules: at import, keep each sanctioned extension hook's content as raw bytes plus in-scope namespace declarations, owned by its host entity; parse `fx` elements of a version defined in the CORE extension schema into typed values (generated table), keep unknown versions opaque with warning `fx_version_unknown`; enforce a 16 MiB per-bag cap (`limit_exceeded`). At export, report `extension_dropped_with_host` for bags whose host was deleted and `dangling_extension_ref` for any UUID in bag content equal to a deleted entity's QPId, keeping the content. Output must be equal to input under `qif-norm@1`. CONSTITUTION §8: no panics, no unwrap; BTreeMap only. Tests AC-1..AC-6.

## Open questions

- Integer `id` references inside opaque vendor content cannot be detected reliably; only QPId references are reported. Whether incumbents keep such content on their own round trips is [unverified] (carried from IO.QIF.IDS).

## Children

_Leaf: none._
