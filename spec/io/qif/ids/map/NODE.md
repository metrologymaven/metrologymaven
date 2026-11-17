---
id: IO.QIF.IDS.MAP
title: QIF identifier map, minting and export renumbering
level: 4
parent: IO.QIF.IDS
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.QIF.IDS-001, REQ-IO.QIF.IDS-002, REQ-IO.QIF.IDS-003]
covers: []
depends_on: [IO.QIF.GEN.RUST, CORE.PRJ.CONT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.QIF.IDS.MAP: QIF identifier map, minting and export renumbering

## Purpose

The `IdMap` that ties each QIF entity's integer `id` and QPId to the platform entity ID, and the rules every IO importer and exporter follow: imported identifiers are kept forever, new entities get UUIDv7 QPIds, derived ones UUIDv5, and on export new entities get integer `id`s above the largest imported one, without ever touching an imported `id`.

## Scope

**In:**
- `IdMap` per imported QIF document: integer `id` → (QPId if present, platform `EntityId`)
- Platform ID rule for imported entities: the QPId when present; otherwise a UUIDv5 derived from the document QPId (or source digest) and the integer `id`, so re-importing the same file gives the same IDs
- Minting for new and derived entities through the CORE ID service
- Export renumbering: deterministic allocation of integer `id`s to entities created after import
- Duplicate and exhausted-ID detection

**Out:**
- Extension content (`IO.QIF.IDS.EXT`) and cross-document `xId` references (`IO.QIF.IDS.XREF`)
- The ID service itself (`CORE.PRJ.CONT`)
- Serialisation (`IO.QIF.XML`, `IO.QIF.JSON`)

## Requirements

- **REQ-IO.QIF.IDS.MAP-001** When a QIF document is imported, every entity's integer `id` and QPId shall be stored in the `IdMap` and written back unchanged on every later export.
- **REQ-IO.QIF.IDS.MAP-002** When an imported entity has a QPId, its platform entity ID shall equal that QPId; when it has none, its platform ID shall be the UUIDv5 of the document QPId (or, failing that, the source SHA-256) and its integer `id`.
- **REQ-IO.QIF.IDS.MAP-003** When a document is exported that contains entities created after import, the system shall give them integer `id`s starting at one more than the largest imported `id`, in ascending order of their UUIDv7 entity IDs.
- **REQ-IO.QIF.IDS.MAP-004** Exporting the same document state twice shall assign the same integer `id`s.
- **REQ-IO.QIF.IDS.MAP-005** New entities shall receive UUIDv7 QPIds and derived entities UUIDv5 QPIds from the CORE ID service, and the map shall never mint an ID itself.
- **REQ-IO.QIF.IDS.MAP-006** If an imported document repeats an integer `id` or a QPId, then the import shall fail with `qif_duplicate_id` naming the value and both positions.
- **REQ-IO.QIF.IDS.MAP-007** If allocation would exceed the maximum value of the QIF integer `id` type, then the export shall fail with `qif_id_exhausted`.

## Acceptance criteria

- **AC-1** Given `fixtures/io/qif/ids/map/ids_basic.qif` (ids 1..12, QPIds on 3 entities), when imported and exported unchanged, then all 12 `id`s and 3 QPIds are identical in the output.
- **AC-2** Given the same import plus two new entities created in order, when exported, then they receive ids 13 and 14 in creation order, and a second export gives the same ids.
- **AC-3** Given `fixtures/io/qif/ids/map/no_qpid.qif`, when imported twice into two empty projects, then each entity has the same platform ID in both.
- **AC-4** (error path) Given `fixtures/io/qif/ids/map/dup_id.qif` with `id="7"` twice, then import fails `qif_duplicate_id` with both line numbers.
- **AC-5** (error path) Given an imported maximum `id` equal to the type's maximum and one new entity, then export fails `qif_id_exhausted`.
- **AC-6** Given an entity deleted after import, when exported, then no other entity's `id` changes.

## Design

- **Registry commands:** none (library used by QIF import and export commands).
- **Data structures:** `IdMap { doc_qpid: Option<Uuid>, source: Sha256, by_int: BTreeMap<u32, MapEntry>, by_qpid: BTreeMap<Uuid, u32> }`; `MapEntry { qpid: Option<Uuid>, entity: EntityId, origin: Imported|Created }`. Integer width follows the generated QIF id type [unverified: `xs:unsignedInt` in QIF 3.0].
- **Algorithm:** build during import (duplicate check); on export, collect created entities, sort by UUIDv7, allocate from max imported + 1; persist allocations in the map so they are stable after the first export as well.
- **Errors:** `qif_duplicate_id`, `qif_id_exhausted`.
- **Determinism:** D0 (UUIDv7 values are recorded inputs; sorting and allocation are pure).
- **Performance budget:** 10^6 entities mapped in < 1 s [unverified until measured].
- **Crate:** `fid-qif` (module `ids::map`).

## Test fixtures

Hand-written QIF 3.0 documents under `fixtures/io/qif/ids/map/` (CC-BY-4.0).

## Build prompt

> In crate `fid-qif`, add module `ids::map` with `IdMap`: record each imported entity's integer `id` and QPId (fail `qif_duplicate_id` on repeats, with both positions); platform entity ID = QPId when present, else UUIDv5(document QPId or source SHA-256, integer id) via the CORE ID service; on export, allocate ids for created entities from max imported + 1 in ascending UUIDv7 order, persist allocations, never change imported ids, fail `qif_id_exhausted` past the id type's maximum. Never mint IDs locally: call `IdService::new_v7/derive_v5`. BTreeMap only; CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-6.

## Open questions

- None.

## Children

_Leaf: none._
