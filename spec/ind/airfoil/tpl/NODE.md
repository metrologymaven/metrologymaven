---
id: IND.AIRFOIL.TPL
title: Blade section report template
level: 3
parent: IND.AIRFOIL
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.AIRFOIL-001]
covers: []
depends_on: [IND.AIRFOIL.PARAMS, IND.AIRFOIL.FORM, IND.AIRFOIL.STACK, RPT]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# IND.AIRFOIL.TPL: Blade section report template

## Purpose

The report template that presents airfoil results: per-section parameter table, zone deviation plot, and stack plots of bow, lean and twist along the span. Registered through the pack manifest using only the public RPT template features.

## Scope

**In:**
- Template registration through the manifest
- Section parameter table, zone deviation plot, stack plots
- Presentation of `NOT_DECIDABLE` and bound-limited results

**Out** (and which node owns it instead):
- The report engine and template language (owned by `RPT`)
- Values shown (owned by `IND.AIRFOIL.PARAMS`, `IND.AIRFOIL.FORM`, `IND.AIRFOIL.STACK`)

## Requirements

- **REQ-IND.AIRFOIL.TPL-001** The blade section template shall be registered only through the airfoil pack manifest and shall use only public RPT template features.
- **REQ-IND.AIRFOIL.TPL-002** The template shall show, per section, every evaluated parameter with nominal, measured, deviation, tolerance and decision, and the method ID of each edge-radius definition used.
- **REQ-IND.AIRFOIL.TPL-003** The template shall mark `NOT_DECIDABLE` and bound-limited results visibly and shall never show a blank where a value was not evaluated.

## Interfaces

- **Offers:** report template `pack.airfoil.blade_section`.
- **Consumes:** RPT template registration; airfoil ResultRecords.

## Decomposition hints

Likely a single leaf once RPT's template format exists.

## Open questions

- None.

## Children

_None yet: status `open`._
