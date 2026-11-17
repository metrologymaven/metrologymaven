---
id: GDT.PROF.LINE
title: Profile of a line
level: 3
parent: GDT.PROF
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.PROF-001, REQ-GDT.PROF-006]
covers: [GOM.GDT.11, PW.GDT.05]
depends_on: [GDT.PROF.DEV, GDT.PROF.ZDEF, GDT.ZONE.POSE]
cli: [fid check eval-line-profile]
mcp: [check_eval_line_profile]
journal: [check.eval_line_profile@1]
updated: 2026-11-16
---

# GDT.PROF.LINE: Profile of a line

## Purpose

Evaluates profile of a line: line elements of the surface, taken in stated section planes, must lie within the 2D profile zone around the nominal curve, with or without datums. It needs line-element extraction (sections through the measured data and the nominal) on top of the surface-profile machinery, which is why it is separate from SURF.

## Scope

**In:**
- Section planes per the chain (direction, positions), section of the nominal faces and selection of measured points per section
- Per-section signed deviations, zone intervals, value; worst section
- With datums: pose fixed or constrained by the DRF; without datums: form control with free pose

**Out** (and which node owns it instead):
- Deviations and zones (owned by `GDT.PROF.DEV`, `GDT.PROF.ZDEF`)
- Best-fit options (owned by `GDT.PROF.BESTFIT`)

## Requirements

- **REQ-GDT.PROF.LINE-001** The system shall evaluate profile of a line in each section the chain names and report the profile value, signed minimum and maximum deviations per section, and the zone used.
- **REQ-GDT.PROF.LINE-002** The characteristic value shall be the worst section value, and every section shall be listed with its plane.
- **REQ-GDT.PROF.LINE-003** Values shall match every ORS-GDT line-profile case within its stated tolerance.

## Interfaces

- **Offers:** `check.eval_line_profile@1` (W2, dry-run R1).
- **Consumes:** siblings DEV and ZDEF; pose solve (GDT.ZONE.POSE).

## Decomposition hints

SECT (section extraction of nominal and points), EVAL (per-section value).

## Open questions

- Default section spacing when the drawing is silent is DMT content.

## Children

_None yet: status `open`._
