---
id: IO.QIF.IDS
title: Identifier preservation and extension pass-through
level: 3
parent: IO.QIF
kind: module
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-IO.QIF-010, REQ-IO.QIF-011]
covers: []
depends_on: [IO.QIF.GEN]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.QIF.IDS: Identifier preservation and extension pass-through

## Purpose

Guarantees that QPIds, QIF `id` and `xId` values survive import, edit and export unchanged, that new entities get IDs by the CONSTITUTION §5 rules, and that content inside QIF's sanctioned extension hooks (`Attributes`, `AttributeUser`, `UserDataXML`) is carried through even when the platform does not model it, including the `fx` extension elements.

## Scope

**In:**
- ID mapping between QIF integer `id`s, QPIds (UUIDs) and platform entity IDs
- Renumbering policy for integer `id`s on export when new entities are added (existing ones never change)
- Opaque, byte-faithful carriage of unknown extension content
- Typed carriage of `fx` extension elements defined by CORE
- Cross-document `xId` references

**Out:**
- The `fx` catalogue contents (CORE data model)
- Serialisation (`IO.QIF.XML`, `IO.QIF.JSON`)

## Requirements

- **REQ-IO.QIF.IDS-001** Imported QPIds and QIF `id` values shall be preserved on every entity and shall be exported unchanged.
- **REQ-IO.QIF.IDS-002** When an exported document contains entities created after import, the system shall assign them integer `id`s greater than the largest imported `id` and shall not change any imported `id`.
- **REQ-IO.QIF.IDS-003** New entities shall receive UUIDv7 QPIds and derived entities UUIDv5 QPIds, as CONSTITUTION §5 requires.
- **REQ-IO.QIF.IDS-004** Content in sanctioned extension hooks that the system does not model shall be preserved on round trip and equal after `qif-norm@1`.
- **REQ-IO.QIF.IDS-005** If an edit deletes an entity that unmodelled extension content refers to, then the system shall report the dangling reference in the export report rather than drop the extension content silently.

## Interfaces

- **Offers:** `IdMap`, `ExtensionBag` types used by `IO.QIF.XML` and `IO.QIF.JSON`; ID minting rules for every IO importer.
- **Consumes:** CORE ID minting (UUIDv7, UUIDv5); `fx` catalogue (CORE).

## Decomposition hints

MAP (ID mapping and renumbering), EXT (extension bag), XREF (cross-document references).

## Open questions

- Whether incumbents keep `Attributes` and `UserDataXML` on QIF round trips needs empirical tests [unverified]; affects how this node reports third-party loss (carried into `IO.QIF.IDS.EXT`).
- The `fx` extension schema is read as a published artifact of `CORE.PRJ.QIF` (`uses_artifacts`), not a code dependency.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| IO.QIF.IDS.MAP | QIF identifier map, minting and export renumbering (leaf) | 001 002 003 | none |
| IO.QIF.IDS.EXT | Extension-hook pass-through and fx typed carriage (leaf) | 004 005 | none |
| IO.QIF.IDS.XREF | Cross-document xId references (leaf) | 001 | none |
