---
id: RPT.BALLOON.CHAR
title: Characteristics from balloons
level: 3
parent: RPT.BALLOON
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.BALLOON-002]
covers: []
depends_on: [RPT.BALLOON.REG, GDT.CHAR]
cli: [fid report add-char-balloon]
mcp: [report_add_char_balloon]
journal: [report.add_char_balloon@1]
updated: 2026-11-16
---

# RPT.BALLOON.CHAR: Characteristics from balloons

## Purpose

When a part has no CAD or PMI, the drawing is the only source of requirements. This module lets the user create a characteristic directly from a balloon (type, nominal, tolerances, unit, dialect) through the characteristic model owned by `GDT.CHAR`, and links the new balloon to it in one step.

## Scope

**In:**
- One command that creates a characteristic through GDT's characteristic commands and places its balloon
- Entry of characteristic type, nominal, tolerances, unit, authored decimals and dialect as authored decimals
- Basic characteristic types for drawing-only work: size, distance, angle, radius, diameter, form and location callouts supported by GDT, and notes (attribute checks)

**Out** (and which node owns it instead):
- Characteristic model, validation and evaluation (`GDT.CHAR`)
- Balloon register (`RPT.BALLOON.REG`)

## Requirements

- **REQ-RPT.BALLOON.CHAR-001** When `report.add_char_balloon@1` runs, the system shall create the characteristic through the `GDT.CHAR` command and add its balloon through `RPT.BALLOON.REG`, as one atomic journaled operation.
- **REQ-RPT.BALLOON.CHAR-002** If the characteristic fails GDT validation, then neither the characteristic nor the balloon shall be created and the GDT error shall be returned unchanged.
- **REQ-RPT.BALLOON.CHAR-003** Nominal and tolerance values shall be kept as authored decimals with their unit (CONSTITUTION §4).
- **REQ-RPT.BALLOON.CHAR-004** The characteristic dialect (ASME or ISO) shall be stated explicitly for each characteristic, with no silent default.

## Interfaces

- **Offers:** `report.add_char_balloon@1` (W2).
- **Consumes:** characteristic creation (`GDT.CHAR`), balloon register (`RPT.BALLOON.REG`), atomic multi-command transactions (CORE).

## Decomposition hints

Probably a single leaf once `GDT.CHAR` names its create command.

## Open questions

- Attribute (pass/fail) characteristics from drawing notes may not exist in GDT's model yet; confirm with `GDT.CHAR`.

## Children

_None yet: status `open`._
