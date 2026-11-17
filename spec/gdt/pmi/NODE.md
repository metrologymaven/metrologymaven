---
id: GDT.PMI
title: PMI to characteristics
level: 2
parent: GDT
kind: subsystem
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT-006]
covers: [GOM.GDT.16, PCD.CAD.02, SA.GDT.01]
depends_on: [GDT.CHAR, GDT.RES, IO]
cli: [fid gdt propose-*-from-pmi, fid gdt adopt-*]
mcp: [gdt_propose_*_from_pmi, gdt_adopt_*]
journal: [gdt.propose_*_from_pmi, gdt.adopt_*]
updated: 2026-11-16
---

# GDT.PMI: PMI to characteristics

## Purpose

Turns imported semantic PMI (STEP AP242, QIF MBD) into **proposed** characteristics, each with its feature control frame, toleranced features and datums linked to CAD faces, and a governing spec. Proposals are not part of the inspection plan until a human adopts them (CONSTITUTION C5). This is the model-based-definition path that GOM, PC-DMIS and SpatialAnalyzer offer; it is separate because it is a translation-and-review workflow, not an evaluation.

## Scope

**In:**
- Mapping imported semantic PMI entities to the `GDT.CHAR` characteristic model
- Linking PMI to toleranced CAD faces and datum features
- Governing-spec proposal from the PMI's declared standard, or from the plan default when the PMI declares none (flagged as inherited)
- Flagging PMI the DMT cannot resolve, graphical-only PMI, and missing references
- Proposal set on an agent or draft branch; human review and adoption, per item or as a set
- Re-import diff: changed, added and removed PMI against an adopted plan

**Out** (and which node owns it instead):
- STEP AP242 and QIF parsing (owned by `IO`)
- Measurement path and probing program generation from PMI (owned by `CMM`)
- Characteristic data model (owned by `GDT.CHAR`)
- Resolving to operator chains (owned by `GDT.RES`)

## Requirements

- **REQ-GDT.PMI-001** When semantic PMI is imported, the system shall create one proposed characteristic per PMI tolerance or dimension entity, on a draft or agent branch (W1), never directly in the working plan.
- **REQ-GDT.PMI-002** A proposed characteristic shall count toward the inspection plan only after a human adopts it through `gdt.adopt_characteristics@1` (W2), and the adoption shall be journaled with the adopting user.
- **REQ-GDT.PMI-003** Every proposed characteristic shall keep a link to its source PMI entity ID and CAD face IDs, and that link shall survive adoption.
- **REQ-GDT.PMI-004** If a PMI entity is graphical-only, references missing geometry, or resolves to `UNRESOLVED` or `UNSUPPORTED` in the DMT, then the system shall still list it in the proposal set with the reason and shall not create an evaluable characteristic for it.
- **REQ-GDT.PMI-005** If the PMI declares no governing standard, then the proposal shall take the plan default and mark the governing spec as inherited, so the reviewer sees it before adoption.
- **REQ-GDT.PMI-006** When PMI is re-imported for a revision, the system shall produce a diff (added, changed, removed) against the adopted characteristics and propose changes without modifying adopted ones.
- **REQ-GDT.PMI-007** On the NIST MBE PMI test models [unverified: availability and licence of the corpus], the system shall propose a characteristic for every semantic PMI entity, with the hit rate published as a number.

## Interfaces

- **Offers:** `gdt.propose_characteristics_from_pmi@1` (W1), `gdt.adopt_characteristics@1` (W2, human adoption required for agent-proposed items), proposal-set and diff types.
- **Consumes:** semantic PMI and CAD face IDs (IO, GEO); characteristic model (GDT.CHAR); resolver for resolvability flags (GDT.RES).

## Decomposition hints

MAP (PMI entity to characteristic), LINK (faces and datums), REVIEW (proposal set and adoption), DIFF (re-import).

## Open questions

- Whether an MCP agent may run `propose` on a document it did not import itself, or only on handles to approved import roots (CONSTITUTION §3).

## Split rationale

The dividing principle is **the review workflow's stages**, following the hints: translate PMI into proposals (MAP), keep the trace to the model (LINK), adopt under human control (REVIEW), and handle revisions (DIFF). LINK, REVIEW and DIFF are mechanical and finished as leaves; MAP stays open because the AP242 entity inventory is large and its supported subset follows the DMT row sets.

Alternatives considered:
- **Fold LINK into MAP.** Rejected: the link invariants span MAP, REVIEW and DIFF, so one owner avoids three copies.
- **Fold DIFF into MAP.** Rejected: DIFF compares against adopted state and has its own command.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| GDT.PMI.MAP | Semantic PMI to proposed characteristics | 001 004 005 007 | GOM.GDT.16, PCD.CAD.02, SA.GDT.01 |
| GDT.PMI.LINK | PMI source, face and datum links (leaf) | 003 | none |
| GDT.PMI.REVIEW | Proposal review and human adoption (leaf) | 002 | GOM.GDT.16 |
| GDT.PMI.DIFF | PMI re-import diff (leaf) | 006 | GOM.GDT.16 |
