---
id: GDT.RES.ROWISO
title: ISO GPS DMT row set
level: 3
parent: GDT.RES
kind: module
status: split
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

- **Offers:** ISO rows and ISO defaults profiles; the ISO counterpart map and P1a ISO inventory (STUBS).
- **Consumes:** schema, workflow, inventory format and release gate (`GDT.RES.ROWASME.INV`, `GDT.RES.ROWASME.GATE`), ORS cases, licensed ISO texts (D-016).

## Decomposition hints

STUBS (P1a placeholder generator; a leaf once D-016 settles whether placeholders need a reviewer), then one child per standard family for P1b.

## Open questions

- **Applied D-016 (accepted 2026-11-17):** P1b authors work from project-purchased licences of each cited ISO edition; reviewers hold their own copies or partner access and attest "edition held" per review. Placeholders contain no interpretation, so their reviews use the attestation `not_required_placeholder` that `GDT.RES.ROWFLOW.STATE` allows only for `unsupported` rows with empty chains; they still need two reviewers, because the released-table lint (L5) requires sign-off on every row. That settles the question the STUBS hint raised.
- The schema requires at least one clause reference per row; placeholders cite only the standard's scope clause (see `GDT.RES.ROWISO.STUBS`). Allowing an empty clause list for `unsupported` rows would be cleaner and is a schema question for the RES maintainers.

## Split rationale

P1a machinery (STUBS, a leaf) is separated from the P1b content, which is split per standard family because each family has its own texts, editions, authors and reviewers: ISO 1101 tolerances (GEOTOL), ISO 5459 datums (DATUM), ISO 2692 material requirements (MATREQ), ISO 14405-1 size (SIZE), and the ISO defaults profiles with the ISO 14253-1 decision-rule hint (DPROF). The inventory format and release gate are reused from `GDT.RES.ROWASME` rather than duplicated.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| GDT.RES.ROWISO.STUBS | ISO counterpart map and P1a placeholder rows (leaf) | 001 | none |
| GDT.RES.ROWISO.GEOTOL | ISO 1101 geometrical tolerance rows | 002 003 | none |
| GDT.RES.ROWISO.DATUM | ISO 5459 datum and datum-system rows | 002 003 | none |
| GDT.RES.ROWISO.MATREQ | ISO 2692 material-requirement rows | 002 003 | none |
| GDT.RES.ROWISO.SIZE | ISO 14405-1 size rows | 002 003 | none |
| GDT.RES.ROWISO.DPROF | ISO defaults profiles and decision-rule hint | 002 003 | none |
