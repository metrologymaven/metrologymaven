---
id: UNC.ORS.FLT.RESAMPLE
title: Resampling cases on known surfaces
level: 4
parent: UNC.ORS.FLT
kind: task
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.ORS.FLT-003]
covers: []
depends_on: [UNC.ORS.TRACE, UNC.RDL.GEN]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.ORS.FLT.RESAMPLE: Resampling cases on known surfaces

## Purpose

Cases for resampling scan data onto grids or onto nominal sample positions, on analytic surfaces where the resampled values are known exactly (planes, spheres, cylinders sampled at known positions). It stays open because no node yet defines a resampling command to name as aim.

## Scope

**In:**
- Resampling of analytic surfaces onto regular grids and onto nominal positions
- Expected resampled coordinates and interpolation error bounds

**Out:**
- Outlier cases (owned by `UNC.ORS.FLT.OUTLIER`); filters (owned by `UNC.ORS.FLT.FILTER`)

## Requirements

- **REQ-UNC.ORS.FLT.RESAMPLE-001** Each resampling method offered by the resampling command shall have at least one case on an analytic surface with closed-form expected values and tolerance.
- **REQ-UNC.ORS.FLT.RESAMPLE-002** Every case shall cite the clause it exercises, or carry `citations: none` with basis `closed_form_math` when the method is project-defined.

## Research hypotheses

- **H-1 (bound validity)** On 100 % of cases the deviation of the 50-digit reference resampling from the analytic surface stays within the stated interpolation error bound, and the bound is within a factor of 2 of the observed maximum [unverified]; falsified otherwise.
- **H-2 (discrimination)** Cases detect at least 95 % [unverified] of seeded single faults (nearest-neighbour instead of the declared interpolation, off-by-one grid origin, transposed grid axes), measured by mutation runs; falsified if fewer.
- **Reference-set gap:** no public reference set for scan resampling was found [unverified]; the generated cases (CC-BY-4.0) are the public reference.

## Interfaces

- **Offers:** cases for suite `ors-flt` in `ors-case/v1` format.
- **Consumes:** case format and checker (UNC.ORS.TRACE); generators (UNC.RDL.GEN).

## Decomposition hints

One generator leaf once the resampling command is named.

## Open questions

- No node owns a resampling command yet (search found only this suite and `GDT.FORM` mentions); SCAN's or GDT's owners need to place it (cross-domain issue).

## Children

_None yet: status `open`._
