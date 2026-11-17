---
id: OUTR.PROG.DATA
title: Teaching datasets
level: 3
parent: OUTR.PROG
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.PROG-006]
covers: []
depends_on: [ECO.DATA]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.PROG.DATA: Teaching datasets

## Purpose

The datasets offered to universities, selected from or added through ECO.DATA, each with a licence that allows redistribution for teaching, a recorded origin and a digest. It is separate because licence checking for teaching use is a distinct responsibility from building the datasets.

## Scope

**In:**
- Teaching dataset list with licence, origin, digest and suitable kits
- Licence check for teaching redistribution
- Download packaging per dataset (through ECO.DATA)

**Out** (and which node owns it instead):
- Creating and hosting datasets (owned by `ECO.DATA`)
- Teaching kits (owned by `OUTR.PROG.KIT`)

## Requirements

- **REQ-OUTR.PROG.DATA-001** Every teaching dataset shall state its licence (SPDX ID where one exists), origin and SHA-256 digest on its listing.
- **REQ-OUTR.PROG.DATA-002** If a dataset's licence does not allow redistribution for teaching, then the listing check shall fail and name the dataset.
- **REQ-OUTR.PROG.DATA-003** If a downloaded dataset's digest differs from its listing, then the listing check shall fail.
- **REQ-OUTR.PROG.DATA-004** Synthetic datasets shall record the generator, version and seed that reproduce them.

## Interfaces

- **Offers:** teaching dataset listing.
- **Consumes:** ECO.DATA datasets and metadata.

## Decomposition hints

LIST (listing and checks), SYNTH (synthetic teaching sets).

## Open questions

- Which licences count as allowing teaching redistribution should follow the D-001 allowlist plus CC-BY-4.0; confirm with ECO.RULES.

## Children

_None yet: status `open`._
