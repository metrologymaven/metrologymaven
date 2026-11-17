---
id: OUTR.DOCS.BOOK
title: User guide (the book)
level: 3
parent: OUTR.DOCS
kind: module
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.DOCS-005]
covers: []
depends_on: [OUTR.DOCS.FRAME]
uses_artifacts: [ECO.APPS.LIST, ECO.PARITY.GEN, ECO.REL.PKG, ECO.DATA.EX]
cli: []
mcp: []
journal: []
updated: 2026-11-17
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

- REQ-OUTR.DOCS.BOOK-004 reuses the OUTR.SITE.CLAIM capability tags, plus a planned-capability tag with a fixed label (OUTR.DOCS.BOOK.CHECK).
- CONSTITUTION §2.1 (D-034): the former code dependency on ECO.APPS is now `uses_artifacts` on the app list file, parity report, release binaries and example projects.
- The app list names Probe and Network from the start; their chapters start in P2 and P3, and stub chapters from OUTR.DOCS.BOOK.CONCEPT keep REQ-OUTR.DOCS.BOOK-001 true until then.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [OUTR.DOCS.BOOK.CHECK](check/NODE.md) | User-guide structure, example and availability check (leaf) | 001 002 003 004 005 | none |
| [OUTR.DOCS.BOOK.CONCEPT](concept/NODE.md) | Concept chapters (leaf) | 001 002 003 004 005 | none |
| [OUTR.DOCS.BOOK.REPORT](report/NODE.md) | Report app chapter (leaf) | 001 003 004 005 | none |
| [OUTR.DOCS.BOOK.INSPECT](inspect/NODE.md) | Inspect app chapter (leaf) | 001 003 004 005 | none |
| [OUTR.DOCS.BOOK.PROBE](probe/NODE.md) | Probe app chapters | 001 003 004 005 | none |
| [OUTR.DOCS.BOOK.NETWORK](network/NODE.md) | Network app chapters | 001 003 004 005 | none |
