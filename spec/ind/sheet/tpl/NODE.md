---
id: IND.SHEET.TPL
title: Sheet metal report template
level: 3
parent: IND.SHEET
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.SHEET-001]
covers: []
depends_on: [IND.SHEET.HOLE, IND.SHEET.TRIM, IND.SHEET.SPRING, IND.SHEET.HEM, RPT]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# IND.SHEET.TPL: Sheet metal report template

## Purpose

The sheet metal report template: hole and trim point tables with in-plane and normal components, springback region summaries with the clamping alignment named, and hem sections. Registered through the pack manifest using only public RPT template features.

## Scope

**In:**
- Template registration through the manifest
- Element tables, region summaries, hem section views, not-measured marking

**Out** (and which node owns it instead):
- Report engine (owned by `RPT`)
- Values shown (owned by the other `IND.SHEET` children)

## Requirements

- **REQ-IND.SHEET.TPL-001** The sheet metal template shall be registered only through the sheet pack manifest and shall use only public RPT template features.
- **REQ-IND.SHEET.TPL-002** The template shall show trim and hole deviations as separate in-plane and normal components, and shall name the clamping alignment on every springback summary.
- **REQ-IND.SHEET.TPL-003** The template shall mark not-measured elements and `NOT_DECIDABLE` values visibly.

## Interfaces

- **Offers:** report template `pack.sheet.inspection`.
- **Consumes:** RPT template registration; sheet ResultRecords.

## Decomposition hints

Likely a single leaf once RPT's template format exists.

## Open questions

- None.

## Children

_None yet: status `open`._
