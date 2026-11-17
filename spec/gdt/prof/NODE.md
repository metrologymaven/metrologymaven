---
id: GDT.PROF
title: Profile and runout tolerances
level: 2
parent: GDT
kind: subsystem
status: open
phase: P1b
size: XL
owners: []
claimed_by:
satisfies: [REQ-GDT-004, REQ-GDT-007]
covers: [GOM.GDT.11, GOM.GDT.12, GOM.GDT.13, PW.GDT.05, PW.GDT.06, PCD.DIM.06]
depends_on: [GDT.ZONE, GDT.DRF, GDT.RES, GEO]
cli: [fid check *-profile, fid check *-runout]
mcp: [check_*_profile, check_*_runout]
journal: [check.*_profile, check.*_runout]
updated: 2026-11-16
---

# GDT.PROF: Profile and runout tolerances

## Purpose

Evaluates the tolerances whose toleranced element is a surface or line element measured directly against a nominal geometry rather than a derived feature: profile of a line, profile of a surface (with or without datums, uniform and non-uniform / unequally disposed zones, best-fit options), and circular and total runout about a datum axis. Both families need closest-point projection onto nominal geometry (analytic or CAD) and per-point signed deviations, which is why they are grouped. Both sit after location on the scope ladder (concept doc 05 §2.5), hence P1b.

## Scope

**In:**
- Profile of a line and profile of a surface, with datums (orientation / location control) and without datums (form control)
- Uniform, unilateral, unequally disposed and non-uniform profile zones
- Profile best-fit options: free best fit within the zone, constrained to DRF DOFs, and with datum mobility (via `GDT.ZONE`)
- Composite and multi-segment profile (tiers solved by `GDT.ZONE`)
- Circular runout and total runout relative to a datum axis (and axial runout to a datum face)
- Per-point signed deviation from nominal used by the zone (projection from GEO and KRN)

**Out** (and which node owns it instead):
- Closest-point projection onto analytic and trimmed NURBS faces (owned by `GEO` and `KRN`)
- Colour deviation maps and whiskers (owned by `SCAN` and `VIZ`)
- Zone solve and composite tiers (owned by `GDT.ZONE`)
- DRF (owned by `GDT.DRF`)
- Airfoil and other industry-specific profile analyses (owned by `IND`)

## Requirements

- **REQ-GDT.PROF-001** The system shall evaluate profile of a line and profile of a surface from measured points and a nominal geometry, and report the profile value, the signed minimum and maximum deviations, and the zone used.
- **REQ-GDT.PROF-002** The system shall support uniform, unilateral, unequally disposed and non-uniform profile zones as defined by the characteristic, and report which zone definition was applied.
- **REQ-GDT.PROF-003** When a profile check offers best-fit options, the system shall evaluate the option selected (none, free, constrained to named DRF DOFs) and record it and the resulting transform in the ResultRecord.
- **REQ-GDT.PROF-004** The system shall evaluate circular and total runout about a datum axis from points and report the runout value and the radial (or axial) extremes per circle or overall.
- **REQ-GDT.PROF-005** If a nominal face's projection carries a nominal approximation error from STEP-to-QIF conversion, then the system shall record that error with the result so UNC can include it as a contributor.
- **REQ-GDT.PROF-006** Profile and runout values shall match every ORS-GDT profile case and ORS-PROJ projection case within the case's stated tolerance.

## Interfaces

- **Offers:** `check.eval_line_profile@1`, `check.eval_surface_profile@1`, `check.eval_circular_runout@1`, `check.eval_total_runout@1` (R1 scratch, W2 when committed).
- **Consumes:** projection onto nominals (GEO, KRN); zone engine (GDT.ZONE); DRF (GDT.DRF); operator chain (GDT.RES); ResultRecord builder (GDT.CHAR).

## Decomposition hints

LINE, SURF, ZONES (non-uniform and unequal), BESTFIT, RUNOUT.

## Open questions

- Grouping runout with profile is a judgement call (shared nominal-deviation machinery); runout could move to `GDT.LOC` if its implementation turns out closer to coaxiality.

## Children

_None yet: status `open`._
