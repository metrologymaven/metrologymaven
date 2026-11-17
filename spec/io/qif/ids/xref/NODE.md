---
id: IO.QIF.IDS.XREF
title: Cross-document xId references
level: 4
parent: IO.QIF.IDS
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-IO.QIF.IDS-001]
covers: []
depends_on: [IO.QIF.IDS.MAP]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.QIF.IDS.XREF: Cross-document xId references

## Purpose

QIF documents can point into other documents (a Results file into its MBD file) through `xId` references paired with a document reference. This leaf keeps such references exactly as written, links them when the target document is also in the project, and never re-mints or rewrites them.

## Scope

**In:**
- `XRef { target_doc: Uuid, xid: u32 }` captured from every cross-document reference
- Linking to loaded documents through their `IdMap`
- Unresolved references kept as unresolved, not as errors
- Export of every reference exactly as imported

**Out:**
- Within-document IDs (`IO.QIF.IDS.MAP`)
- Loading the external document (the user imports it with the QIF import commands)

## Requirements

- **REQ-IO.QIF.IDS.XREF-001** Every cross-document reference shall be stored as its target document QPId and `xId` and exported unchanged.
- **REQ-IO.QIF.IDS.XREF-002** When the target document is present in the project, the reference shall be linked to the target's platform entity through that document's `IdMap`.
- **REQ-IO.QIF.IDS.XREF-003** If the target document is present but has no entity with that `xId`, then the import report shall carry warning `xref_unresolved` with both identifiers, and the import shall still succeed.
- **REQ-IO.QIF.IDS.XREF-004** If the target document is absent, then the reference shall stay unresolved without a warning and shall be resolved when that document is imported later.

## Acceptance criteria

- **AC-1** Given `fixtures/io/qif/ids/xref/results.qif` referencing `mbd.qif`, when only `results.qif` is imported and re-exported, then every reference is byte-equal under `qif-norm@1`.
- **AC-2** Given both files imported (either order), then each reference resolves to the MBD entity with that id.
- **AC-3** Given `fixtures/io/qif/ids/xref/results_bad.qif` referencing `xId="999"` absent from `mbd.qif`, then the import report has `xref_unresolved` and the import succeeds.

## Design

- **Registry commands:** none (library).
- **Data structures:** `XRef { target_doc, xid, resolved: Option<EntityId> }`; per-project index `BTreeMap<Uuid, IdMapRef>` of loaded QIF documents.
- **Algorithm:** capture during binding; after each QIF import, re-scan unresolved references whose `target_doc` equals the new document and link them (a derived index, not a document change).
- **Errors:** warning `xref_unresolved` only.
- **Determinism:** D0.
- **Performance budget:** 10^5 references resolved in < 100 ms [unverified until measured].
- **Crate:** `fid-qif` (module `ids::xref`).

## Test fixtures

`fixtures/io/qif/ids/xref/` hand-written MBD and Results pair (CC-BY-4.0).

## Build prompt

> In crate `fid-qif`, add module `ids::xref`: capture every cross-document reference as (target document QPId, `xId`), export it unchanged, resolve it through the target document's `IdMap` when that document is in the project (re-scan when a document is imported later), and report `xref_unresolved` (warning, import succeeds) when the target document is present but lacks the `xId`. Never re-mint or rewrite references. CONSTITUTION §8: no panics, no unwrap; BTreeMap only. Tests AC-1..AC-3.

## Open questions

- The exact element and attribute names QIF 3.0 uses for document references are to be taken from the pinned XSDs [unverified].

## Children

_Leaf: none._
