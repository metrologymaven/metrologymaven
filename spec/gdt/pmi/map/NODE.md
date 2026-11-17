---
id: GDT.PMI.MAP
title: Semantic PMI to proposed characteristics
level: 3
parent: GDT.PMI
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.PMI-001, REQ-GDT.PMI-004, REQ-GDT.PMI-005, REQ-GDT.PMI-007]
covers: [GOM.GDT.16, PCD.CAD.02, SA.GDT.01]
depends_on: [GDT.CHAR.MODEL, GDT.CHAR.SPEC, GDT.RES.RESOLVE, GDT.PMI.LINK]
uses_artifacts: [IO.CAD.PMI, IO.CORP]
cli: [fid gdt propose-characteristics-from-pmi]
mcp: [gdt_propose_characteristics_from_pmi]
journal: [gdt.propose_characteristics_from_pmi@1]
updated: 2026-11-17
---

# GDT.PMI.MAP: Semantic PMI to proposed characteristics

## Purpose

Maps each imported semantic PMI entity (STEP AP242 geometric tolerances, dimensions, datums; QIF MBD characteristics) to a **proposed** characteristic in the `GDT.CHAR.MODEL` types, with its feature control frame, a governing spec proposal, and resolvability flags from the resolver, written to a draft or agent branch. Entities that cannot become evaluable characteristics (graphical-only PMI, missing references, unresolved or unsupported in the DMT) are still listed with a reason. This is the MBD path the seeds name; the automatic CMM program creation part of PCD.CAD.02 is CMM's.

## Scope

**In:**
- Entity-type mapping table from AP242 and QIF MBD PMI entities to characteristic variants
- Governing-spec proposal from the PMI's declared standard; plan default otherwise, marked inherited
- Resolvability flags from `gdt.resolve_chain@1`
- Proposal-set item per entity with status `evaluable | not_evaluable{reason}`
- `gdt.propose_characteristics_from_pmi@1` (W1, draft or agent branch)
- Hit-rate measurement on the NIST MBE PMI models [unverified: availability and licence]

**Out** (and which node owns it instead):
- STEP and QIF parsing (owned by `IO.CAD`, `IO.QIF`)
- Face and datum links (owned by `GDT.PMI.LINK`)
- Adoption (owned by `GDT.PMI.REVIEW`)
- Re-import diff (owned by `GDT.PMI.DIFF`)
- Program generation from PMI (owned by `CMM.GEN`)

## Requirements

- **REQ-GDT.PMI.MAP-001** When semantic PMI is imported, the system shall create exactly one proposal item per PMI tolerance or dimension entity on a draft or agent branch, never in the working plan.
- **REQ-GDT.PMI.MAP-002** If an entity is graphical-only, references missing geometry, or resolves `UNRESOLVED` or `UNSUPPORTED`, then its item shall have status `not_evaluable` with the reason, and no evaluable characteristic shall be created.
- **REQ-GDT.PMI.MAP-003** If the PMI declares no governing standard, then the item's governing spec shall be the plan default marked `inherited_from: plan`.
- **REQ-GDT.PMI.MAP-004** On each corpus model used, the proposal hit rate (items created / semantic PMI entities) and the evaluable rate shall be published as numbers.

## Interfaces

- **Offers:** `gdt.propose_characteristics_from_pmi@1` (W1); `ProposalSet` type.
- **Consumes:** PMI entities and face IDs (IO.CAD, IO.QIF); characteristic model and spec (GDT.CHAR); resolver (GDT.RES.RESOLVE); links (GDT.PMI.LINK).

## Decomposition hints

TABLE (entity-type mapping per AP242 and QIF), SPEC (governing-spec proposal), FLAGS (resolvability and reasons), CMD (proposal command), CORPUS (hit-rate measurement).

## Open questions

- Inherited from `GDT.PMI`: whether an MCP agent may run `propose` on a document it did not import itself, or only on handles to approved import roots (CONSTITUTION §3). `GDT.PMI.MAP.CMD` accepts handles to approved roots only.
- Resolved at split: the AP242 entity inventory is mapped to QIF MBD by `IO.CAD.PMI`, so this module consumes QIF MBD characteristics plus IO's import report (read as an artifact) and reuses the characteristic QIF mapping; the supported subset follows the DMT through resolvability flags, with no separate P1b entity list.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| GDT.PMI.MAP.ITEM | PMI entity to proposal item with evaluability status (leaf) | 001 002 | GOM.GDT.16, SA.GDT.01 |
| GDT.PMI.MAP.CMD | Propose characteristics from PMI command (leaf) | 001 003 | GOM.GDT.16, PCD.CAD.02, SA.GDT.01 |
| GDT.PMI.MAP.CORPUS | PMI proposal hit-rate measurement (leaf) | 004 | PCD.CAD.02 |
