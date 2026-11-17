---
id: IND.PORT.TPL
title: Routine report template
level: 3
parent: IND.PORT
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.PORT-001]
covers: []
depends_on: [IND.PORT.GUIDE, RPT]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# IND.PORT.TPL: Routine report template

## Purpose

The routine report template: per-step results with expected and recognised feature types, operator overrides, attempt counts and check outcomes, so a reviewer sees how the measurement was taken. Registered through the pack manifest using only public RPT template features.

## Scope

**In:**
- Template registration through the manifest
- Step table with overrides and retakes visible

**Out** (and which node owns it instead):
- Report engine (owned by `RPT`)
- Values shown (owned by `IND.PORT.GUIDE`)

## Requirements

- **REQ-IND.PORT.TPL-001** The routine template shall be registered only through the portable pack manifest and shall use only public RPT template features.
- **REQ-IND.PORT.TPL-002** The template shall show, per step, the expected and recognised feature types, any operator override, the number of attempts and the acceptance check outcome.

## Interfaces

- **Offers:** report template `pack.portable.routine`.
- **Consumes:** RPT template registration; routine run records.

## Decomposition hints

Likely a single leaf once RPT's template format exists.

## Open questions

- None.

## Children

_None yet: status `open`._
