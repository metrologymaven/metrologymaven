---
id: OUTR.DOCS.BOOK.NETWORK
title: Network app chapters
level: 4
parent: OUTR.DOCS.BOOK
kind: feature
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.DOCS.BOOK-001, REQ-OUTR.DOCS.BOOK-003, REQ-OUTR.DOCS.BOOK-004, REQ-OUTR.DOCS.BOOK-005]
covers: []
depends_on: [OUTR.DOCS.BOOK.CHECK]
uses_artifacts: [ECO.DATA.EX, ECO.REL.PKG, ECO.PARITY.GEN]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.DOCS.BOOK.NETWORK: Network app chapters

## Purpose

The user-guide chapters for the Network app: instrument stations, network adjustment, transformations, build guidance and reports with uncertainty. They start when Network's core workflow exists (P3).

## Scope

**In:**
- Chapters with `app: network`, runnable examples on recorded instrument data, capability tags

**Out** (and which node owns it instead):
- Network tutorials (owned by `OUTR.CONTENT.TUT.NETWORK`)
- The SpatialAnalyzer migration guide (owned by `OUTR.CONTENT.MIG.SA`)

## Requirements

- **REQ-OUTR.DOCS.BOOK.NETWORK-001** When the Network app is in the app list of the documented release, at least one chapter with `app: network` shall pass `book-check`.
- **REQ-OUTR.DOCS.BOOK.NETWORK-002** Every runnable example in the Network chapters shall run on recorded instrument observations only and pass `book-check --run`.
- **REQ-OUTR.DOCS.BOOK.NETWORK-003** The chapters shall state the uncertainty status of every adjusted result they show.

## Interfaces

- **Offers:** Network chapters of the user guide.
- **Consumes:** `book-check`; ECO example projects; release binaries.

## Decomposition hints

NET (stations and adjustment), XFM (transformations), BUILD (build guidance and reports).

## Open questions

- Not in launch-1. Note: REQ-OUTR.DOCS.BOOK-001 requires a chapter for every app in the app list, which lists Network from the start; until this node is a leaf, the stub chapter required by REQ-OUTR.DOCS.BOOK.CONCEPT-006 satisfies the check.

## Children

_None yet: status `open`._
