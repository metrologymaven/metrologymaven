---
id: IND.TUBE.TPL
title: Tube report template
level: 3
parent: IND.TUBE
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.TUBE-001]
covers: []
depends_on: [IND.TUBE.CMP, IND.TUBE.CORR, RPT]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# IND.TUBE.TPL: Tube report template

## Purpose

The tube report template: measured and nominal bend tables in XYZ and LRA, deviations with tolerances, end deviations, envelope, and advisory corrections clearly separated from results. Registered through the pack manifest using only public RPT template features.

## Scope

**In:**
- Template registration through the manifest
- Bend tables, deviation tables, advisory correction block

**Out** (and which node owns it instead):
- Report engine (owned by `RPT`)
- Values shown (owned by the other `IND.TUBE` children)

## Requirements

- **REQ-IND.TUBE.TPL-001** The tube template shall be registered only through the tube pack manifest and shall use only public RPT template features.
- **REQ-IND.TUBE.TPL-002** The template shall show measured and nominal bend data in both XYZ and LRA, with deviations, tolerances and decisions.
- **REQ-IND.TUBE.TPL-003** The template shall show corrections in a block labelled advisory, visually separate from results of record.

## Interfaces

- **Offers:** report template `pack.tube.inspection`.
- **Consumes:** RPT template registration; tube ResultRecords and corrections.

## Decomposition hints

Likely a single leaf once RPT's template format exists.

## Open questions

- None.

## Children

_None yet: status `open`._
