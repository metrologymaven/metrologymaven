---
id: IND.GAPFL.TPL
title: Gap and flush report template
level: 3
parent: IND.GAPFL
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.GAPFL-001]
covers: []
depends_on: [IND.GAPFL.SECT, IND.GAPFL.JOINT, RPT]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# IND.GAPFL.TPL: Gap and flush report template

## Purpose

The gap and flush report template: per-point gap and flush with definitions and master panel named, and per-joint station charts with maximum, minimum and range. Registered through the pack manifest using only public RPT template features.

## Scope

**In:**
- Template registration through the manifest
- Point tables, joint charts, missing-edge marking

**Out** (and which node owns it instead):
- Report engine (owned by `RPT`)
- Values shown (owned by the other `IND.GAPFL` children)

## Requirements

- **REQ-IND.GAPFL.TPL-001** The gap and flush template shall be registered only through the pack manifest and shall use only public RPT template features.
- **REQ-IND.GAPFL.TPL-002** The template shall name, for every value, the gap or flush definition, the master panel and the evaluation basis.
- **REQ-IND.GAPFL.TPL-003** The template shall mark `NOT_DECIDABLE` stations and name the missing edge.

## Interfaces

- **Offers:** report template `pack.gapflush.joints`.
- **Consumes:** RPT template registration; gap and flush ResultRecords.

## Decomposition hints

Likely a single leaf once RPT's template format exists.

## Open questions

- None.

## Children

_None yet: status `open`._
