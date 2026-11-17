---
id: GDT.PMI.LINK
title: PMI source, face and datum links
level: 3
parent: GDT.PMI
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.PMI-003]
covers: []
depends_on: [GDT.CHAR.MODEL]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GDT.PMI.LINK: PMI source, face and datum links

## Purpose

Every characteristic proposed from PMI keeps a link to where it came from: the source PMI entity ID, the toleranced CAD face IDs and the datum feature face IDs. The link must survive adoption, edits and QIF export so that a reviewer, a re-import diff or a verifier can trace a characteristic back to the model. This leaf owns the link type and its preservation rules.

## Scope

**In:** `PmiLink { source_file_digest, pmi_entity_id, toleranced_faces, datum_faces: [(label, faces)] }`; attaching it to proposal items and characteristics; preserving it through adoption and edits; writing it to QIF through the `fx:` attribute hooks and reading it back.
**Out:** creating proposals (owned by `GDT.PMI.MAP`); adoption (owned by `GDT.PMI.REVIEW`); CAD face identity (owned by `GEO.CAD` and `IO.CAD`).

## Requirements

- **REQ-GDT.PMI.LINK-001** Every proposal item created from PMI shall carry a `PmiLink` with source file digest, PMI entity ID and the toleranced face IDs; datum faces shall be included when the PMI references datums.
- **REQ-GDT.PMI.LINK-002** When a proposal item is adopted, the adopted characteristic shall carry the same `PmiLink` unchanged.
- **REQ-GDT.PMI.LINK-003** An edit to a characteristic shall not alter its `PmiLink`; if an edit tries to, it shall be rejected with `pmi_link_immutable`.
- **REQ-GDT.PMI.LINK-004** The link shall round-trip through QIF export and import via `fx:` attributes without loss.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/pmi/link_proposal.json` (one proposal with entity `#1234`, faces `[f7, f9]`, datum A on `f2`), when adopted via the adoption API stub, then the characteristic's link equals the proposal's byte for byte in canonical JSON.
- **AC-2** Given the adopted characteristic, when its tolerance is edited, then the link is unchanged.
- **AC-3** (error path) Given an edit payload with a different entity ID, then `pmi_link_immutable`.
- **AC-4** Given QIF export and re-import (with the IO stub), then the link is equal.

## Design

- **Registry command(s):** none (data type and invariants used by MAP, REVIEW and DIFF).
- **Data structures:** `PmiLink` as in Scope; face IDs are the stable IDs from GEO.CAD.
- **Algorithm:** copy-on-adopt; equality check on edit; serialise to `fx:pmiLink` attributes.
- **Errors:** `pmi_link_immutable`, `malformed_pmi_link`.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-gdt`.

## Test fixtures

Synthetic proposal JSON and a QIF stub (CC0, in-repo).

## Build prompt

> In crate `fid-gdt`, define `PmiLink { source_file_digest, pmi_entity_id, toleranced_faces, datum_faces: Vec<(DatumLabel, Vec<FaceId>)> }` and attach it to proposal items and characteristics. Invariants: copied unchanged on adoption; edits that change it fail with `pmi_link_immutable`; serialised to QIF through `fx:pmiLink` attributes and read back losslessly (`malformed_pmi_link` on bad input). Tests per the acceptance criteria using stubs for adoption and QIF I/O. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`, bounded fail-closed parsing), no std `HashMap`/`HashSet`.

## Open questions

- None.

## Children

_Leaf: none._
