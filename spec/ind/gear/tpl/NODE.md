---
id: IND.GEAR.TPL
title: Gear report template
level: 3
parent: IND.GEAR
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.GEAR-001]
covers: []
depends_on: [IND.GEAR.FLANK, IND.GEAR.PITCH, IND.GEAR.GRADE, RPT]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# IND.GEAR.TPL: Gear report template

## Purpose

The gear report template: per-flank profile and helix traces with evaluation ranges, pitch deviation bar charts, runout and size values, and a grade table. Registered through the pack manifest using only public RPT template features.

## Scope

**In:**
- Template registration through the manifest
- Trace plots, pitch charts, grade table, `NOT_DECIDABLE` marking

**Out** (and which node owns it instead):
- Report engine (owned by `RPT`)
- Values shown (owned by the other `IND.GEAR` children)

## Requirements

- **REQ-IND.GEAR.TPL-001** The gear template shall be registered only through the gear pack manifest and shall use only public RPT template features.
- **REQ-IND.GEAR.TPL-002** The template shall show, per flank, the profile and helix deviation traces with the evaluation range marked, and the total, form and slope values with their tolerances and achieved grades.
- **REQ-IND.GEAR.TPL-003** The template shall show single and cumulative pitch deviations per tooth for left and right flanks and shall mark `NOT_DECIDABLE` values visibly.

## Interfaces

- **Offers:** report template `pack.gear.analysis`.
- **Consumes:** RPT template registration; gear ResultRecords.

## Decomposition hints

Likely a single leaf once RPT's template format exists.

## Open questions

- None.

## Children

_None yet: status `open`._
