---
id: GDT.RES.ROWISO
title: ISO GPS DMT row set
level: 3
parent: GDT.RES
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.RES-009]
covers: []
depends_on: [GDT.RES.SCHEMA, GDT.RES.ROWFLOW]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.RES.ROWISO: ISO GPS DMT row set

## Purpose

The ISO GPS rows of the DMT: ISO 1101, ISO 5459 (2011 legacy and 2024), ISO 2692, ISO 14405-1 (2016 legacy and 2025) and the ISO 14253-1 default decision-rule hint. In P1a this node delivers only **placeholder rows with status `unsupported`** for every ISO key the P1a inventory names, so the resolver and support matrix state the gap honestly; the real rows arrive in P1b under the D-009 authorship model.

## Scope

**In:**
- P1a: `unsupported` placeholder rows for every ISO counterpart of the P1a ASME inventory (no interpretation content, generated from the inventory)
- P1b: expert-authored ISO rows per edition, keyed on each edition's defaults profile
- ISO defaults profiles (`dp:iso-…`) content

**Out** (and which node owns it instead):
- ASME rows (owned by `GDT.RES.ROWASME`)
- Row schema and workflow (owned by `GDT.RES.SCHEMA`, `GDT.RES.ROWFLOW`)
- ISO 14405-1 size evaluation (owned by `GDT.DIM.SIZEISO`)

## Requirements

- **REQ-GDT.RES.ROWISO-001** In P1a, the DMT shall contain an `unsupported` row for the ISO counterpart of every P1a ASME inventory key, with no operator chain beyond an empty placeholder and no clause interpretation.
- **REQ-GDT.RES.ROWISO-002** From P1b, every ISO row shall name the edition set it applies to, and rows for different editions of the same standard shall be distinct keys.
- **REQ-GDT.RES.ROWISO-003** Every ISO row promoted from `unsupported` shall meet the same sign-off and ORS-link gates as `GDT.RES.ROWASME`.

## Interfaces

- **Offers:** ISO rows and ISO defaults profiles.
- **Consumes:** schema, workflow, ORS cases, purchased ISO texts (D-016).

## Decomposition hints

STUBS (P1a placeholder generator; a leaf once D-016 settles whether placeholders need a reviewer), then one child per standard family for P1b.

## Open questions

- **Unblocked (2026-11-17):** D-016 (Acquiring and sharing standards texts for DMT authoring and review) is accepted. Apply its Decision section (`decisions/D-016-*.md`) when this node is expanded.
- **Blocked on D-016** for the P1b rows. The P1a placeholder rows need no standards text and could be split out as a leaf once the decision is taken.

## Children

_None yet: status `decision`._
