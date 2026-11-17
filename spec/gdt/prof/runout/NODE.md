---
id: GDT.PROF.RUNOUT
title: Circular, total and axial runout
level: 3
parent: GDT.PROF
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.PROF-004, REQ-GDT.PROF-006]
covers: [GOM.GDT.13, PW.GDT.06]
depends_on: [GDT.PROF.DEV, GDT.DRF.BUILD]
cli: [fid check eval-circular-runout, fid check eval-total-runout]
mcp: [check_eval_circular_runout, check_eval_total_runout]
journal: [check.eval_circular_runout@1, check.eval_total_runout@1]
updated: 2026-11-16
---

# GDT.PROF.RUNOUT: Circular, total and axial runout

## Purpose

Evaluates runout relative to a datum axis: circular runout (per measuring circle, the full indicator movement of radial or axial deviation), total runout (over the whole surface), and axial runout on faces perpendicular to the axis. The datum axis is fixed by the DRF, so there is no pose search; the value is a range of signed deviations per circle or overall.

## Scope

**In:**
- Circular radial runout per cross-section; circular axial runout per circle on a face
- Total radial and total axial runout over the surface
- Measuring-circle extraction per chain (positions, band width)
- Per-circle extremes and the overall value

**Out** (and which node owns it instead):
- Deviations (owned by `GDT.PROF.DEV`)
- Datum axis (owned by `GDT.DRF.BUILD`)
- Coaxiality (owned by `GDT.LOC.COAX`)

## Requirements

- **REQ-GDT.PROF.RUNOUT-001** The system shall evaluate circular runout as, per measuring circle, the range (max − min) of signed deviations along the radial or axial direction about the datum axis, and report the worst circle and every circle's value.
- **REQ-GDT.PROF.RUNOUT-002** The system shall evaluate total runout as the range of signed deviations over all toleranced points.
- **REQ-GDT.PROF.RUNOUT-003** Values shall match every ORS-GDT runout case within its stated tolerance.

## Interfaces

- **Offers:** `check.eval_circular_runout@1`, `check.eval_total_runout@1` (W2, dry-run R1).
- **Consumes:** deviations (GDT.PROF.DEV); DRF (GDT.DRF.BUILD).

## Decomposition hints

CIRC (circular runout given explicit circles; likely a leaf), TOTAL, AXIAL.

## Open questions

- Whether runout belongs here or with coaxiality in `GDT.LOC` (raised on `GDT.PROF`); kept here because it reuses DEV.

## Children

_None yet: status `open`._
