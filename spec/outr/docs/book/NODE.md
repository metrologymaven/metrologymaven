---
id: OUTR.DOCS.BOOK
title: User guide (the book)
level: 3
parent: OUTR.DOCS
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.DOCS-005]
covers: []
depends_on: [OUTR.DOCS.FRAME, ECO.APPS]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.DOCS.BOOK: User guide (the book)

## Purpose

The user guide: the concepts and workflows a user needs, with one chapter per focused app and cross-cutting chapters on journals and replay, units and result verification. It is authored text, but every command example in it is checked against the release it documents, so it cannot silently drift from the product.

## Scope

**In:**
- Concept chapters: project model, registry and command surfaces, journals and replay, units, uncertainty status and decisions, result verification
- One chapter per focused app of the D-004 family
- Executable command examples checked per release
- Chapter skeleton and ownership list, so contributors can claim chapters

**Out** (and which node owns it instead):
- Step-by-step tutorials and migration guides (owned by `OUTR.CONTENT`)
- Command reference pages (owned by `OUTR.DOCS.REF`)
- Contributor guide (owned by `OUTR.DOCS.CONTRIB`)

## Requirements

- **REQ-OUTR.DOCS.BOOK-001** The user guide shall have at least one chapter for each focused app declared in the ECO.APPS manifest of the release it documents, and the docs build shall fail if an app has none.
- **REQ-OUTR.DOCS.BOOK-002** The user guide shall have one chapter each on journals and replay, units, and result verification with `fid-verify`.
- **REQ-OUTR.DOCS.BOOK-003** Every command example in the user guide marked as runnable shall run against the documented release in an automated check and produce the output shown, within a stated normalisation (for example ignoring timestamps and UUIDs).
- **REQ-OUTR.DOCS.BOOK-004** If a chapter describes a capability that is not live in the release's parity report, then the chapter shall label it as not yet available in that release.
- **REQ-OUTR.DOCS.BOOK-005** Every chapter shall pass the OUTR.BRAND claim lint with zero errors.

## Interfaces

- **Offers:** the user guide pages per release; a chapter list with owners.
- **Consumes:** OUTR.DOCS.FRAME; ECO.APPS manifest; ECO.PARITY report; release binaries for example checks; OUTR.BRAND.LINT.

## Decomposition hints

CONCEPT (cross-cutting chapters), APPCH (one per app), EXAMPLE (runnable-example check).

## Open questions

- REQ-OUTR.DOCS.BOOK-004 reuses the capability-tag idea of OUTR.SITE.CLAIM; the check could be shared if the book uses the same tag syntax.

## Children

_None yet: status `open`._
