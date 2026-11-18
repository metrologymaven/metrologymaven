---
id: GDT.DRF.COMMON.TARGET
title: Datums established from datum targets
level: 4
parent: GDT.DRF.COMMON
kind: feature
status: open
phase: P1b
size: L
owners: []
claimed_by:
needs_expert: gdt
satisfies: [REQ-GDT.DRF.COMMON-002, REQ-GDT.DRF.COMMON-004]
covers: []
depends_on: [GDT.DRF.DATUM, GDT.DRF.COMMON.COVER, GDT.RES.RESOLVE, KRN.ENV]
uses_artifacts: [GDT.RES.ROWASME, GDT.RES.ROWISO, UNC.ORS.DRF]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.DRF.COMMON.TARGET: Datums established from datum targets

## Purpose

Simulates a datum from its datum targets (points, lines, areas) at the target locations stored in the datum definition. Which measured points represent a target contact (for example the nearest point, a local fit inside the target area, or a movable target), and how the target contacts are combined into a plane or axis, are interpretation content of the DMT rows (D-009, D-016). This node executes those rules after `GDT.DRF.COMMON.COVER` has confirmed that every target is covered by data.

## Scope

**In:**
- Target contact extraction per the row's rule for point, line and area targets
- Datum simulation from the target contacts with the row-named fit
- Recording each target's contact points

**Out** (and which node owns it instead):
- Target geometry storage (owned by `GDT.DRF.DATUM`)
- Coverage checks (owned by `GDT.DRF.COMMON.COVER`)
- Common datums (owned by `GDT.DRF.COMMON.CDAT`)

## Requirements

- **REQ-GDT.DRF.COMMON.TARGET-001** The system shall simulate a datum from its targets using the target geometry in the datum definition, the contact rule and the fit named by the resolved row, and record each target's contact points.
- **REQ-GDT.DRF.COMMON.TARGET-002** The system shall run the coverage check before simulation and stop on `target_not_covered`.
- **REQ-GDT.DRF.COMMON.TARGET-003** Target datums shall match every ORS-DRF datum-target case within its stated tolerance.

## Interfaces

- **Offers:** `simulate_from_targets(datum_def, points, chain) -> SimulatedDatum` to `GDT.DRF.BUILD`.
- **Consumes:** datum definitions (GDT.DRF.DATUM); coverage (GDT.DRF.COMMON.COVER); fits (KRN.ENV); rows and ORS-DRF cases (read as data).

## Decomposition hints

POINT (point targets), LINE (line targets), AREA (area targets), MOVABLE (movable targets if a row calls for them).

## Open questions

- Not a leaf: contact rules for targets are DMT content.

## Children

_None yet: status `open`._
