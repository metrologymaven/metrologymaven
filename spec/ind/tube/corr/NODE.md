---
id: IND.TUBE.CORR
title: Bend corrections
level: 3
parent: IND.TUBE
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.TUBE-006]
covers: [PW.IND.04, PCD.IND.04]
depends_on: [IND.TUBE.CMP]
cli: [fid pack compute-tube-corrections]
mcp: [pack_compute_tube_corrections]
journal: [pack.compute_tube_corrections@1]
updated: 2026-11-16
---

# IND.TUBE.CORR: Bend corrections

## Purpose

Computes per-bend LRA corrections that would null the measured deviation on the next part, as advisory derived results for the bender operator. Corrections are never values of record and never affect conformance.

## Scope

**In:**
- First-order correction model: corrected LRA = previous program LRA minus measured LRA deviation
- Optional damping factor and limits per correction
- Advisory marking of every correction
- `pack.compute_tube_corrections@1` (R1)

**Out** (and which node owns it instead):
- The deviations (owned by `IND.TUBE.CMP`)
- Writing corrections to bender files (owned by `IND.TUBE.EXP`)
- Live bender communication (owned by `HAL`; not yet catalogued)

## Requirements

- **REQ-IND.TUBE.CORR-001** When asked for corrections, the pack shall compute per-bend corrections to length, rotation and bend angle that would null the measured LRA deviation under the declared correction model.
- **REQ-IND.TUBE.CORR-002** Every correction shall be marked advisory and shall never be emitted as a value of record or used in a conformance decision.
- **REQ-IND.TUBE.CORR-003** If a correction exceeds its declared limit, then the pack shall clamp it to the limit and flag it as limited.
- **REQ-IND.TUBE.CORR-004** The correction model shall be a versioned method ID recorded with every correction set.

## Interfaces

- **Offers:** `pack.compute_tube_corrections@1` (R1); `TubeCorrections` type for EXP.
- **Consumes:** `IND.TUBE.CMP` deviations.

## Decomposition hints

Likely one leaf once a tube-shop maintainer confirms the correction model.

## Open questions

- Whether springback-aware correction (bend angle overbend as a function of bend angle) is expected in the first release needs a tube-shop expert [unverified].

## Children

_None yet: status `open`._
