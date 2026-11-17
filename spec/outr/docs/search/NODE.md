---
id: OUTR.DOCS.SEARCH
title: Client-side docs search
level: 3
parent: OUTR.DOCS
kind: module
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.DOCS-008]
covers: []
depends_on: [OUTR.DOCS.FRAME]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.DOCS.SEARCH: Client-side docs search

## Purpose

Search over each release edition of the portal that runs entirely in the reader's browser, from an index built at docs build time and shipped with the edition and its offline bundle. It is separate because hosted search services would break the no-third-party and offline rules.

## Scope

**In:**
- Build-time search index per release edition
- In-browser search UI with no network requests beyond the portal's own origin
- Search in the offline bundle
- Command-ID-aware ranking so that typing a command ID finds its reference pages first

**Out** (and which node owns it instead):
- The off-origin request crawl (owned by `OUTR.DOCS.QA`, which reuses `OUTR.SITE.QA`)
- Page content (owned by `OUTR.DOCS.REF`, `OUTR.DOCS.BOOK`, `OUTR.DOCS.CONTRIB`, `OUTR.CONTENT`)

## Requirements

- **REQ-OUTR.DOCS.SEARCH-001** The docs build shall produce one search index per release edition, and the search UI shall query only that edition's index.
- **REQ-OUTR.DOCS.SEARCH-002** When a reader searches, the search shall run entirely in the browser and make no request to any origin other than the portal's own.
- **REQ-OUTR.DOCS.SEARCH-003** Search shall work from the unpacked offline bundle with no network access.
- **REQ-OUTR.DOCS.SEARCH-004** When the query equals a command ID, CLI command or MCP tool name of the edition, the matching reference pages shall be the first results.
- **REQ-OUTR.DOCS.SEARCH-005** The search index of one edition shall stay under a size budget recorded in the docs repository (initial 5 MB [unverified]), and the build shall fail when it is exceeded.

## Interfaces

- **Offers:** search UI and per-edition index.
- **Consumes:** OUTR.DOCS.FRAME pages and edition layout; command names from OUTR.DOCS.REF.

## Decomposition hints

INDEX (index build), UI (search box and results), RANK (command-ID ranking).

## Open questions

- The index library is an implementation choice; it must work without a server and offline.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [OUTR.DOCS.SEARCH.INDEX](index/NODE.md) | Per-edition search index with command-name table (leaf) | 001 003 004 005 | none |
| [OUTR.DOCS.SEARCH.UI](ui/NODE.md) | In-browser search page with no network requests (leaf) | 001 002 003 004 | none |
