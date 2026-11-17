---
id: IND.TUBE.CMP
title: Measured versus nominal tube comparison
level: 3
parent: IND.TUBE
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.TUBE-004]
covers: [PW.IND.04, PCD.IND.04]
depends_on: [IND.TUBE.MODEL, IND.TUBE.RECON, IND.TUBE.BEND, ALN]
cli: [fid pack evaluate-tube]
mcp: [pack_evaluate_tube]
journal: [pack.evaluate_tube@1]
updated: 2026-11-16
---

# IND.TUBE.CMP: Measured versus nominal tube comparison

## Purpose

Compares a reconstructed measured tube with its nominal: per-straight length deviation, per-bend rotation and angle deviation, and end-point and end-direction deviation, under a declared best-fit constraint (ends fixed, one end fixed, or free), plus an envelope check of the centreline against nominal.

## Scope

**In:**
- Matching measured to nominal straights and bends by index
- Best fit of the measured centreline to nominal under declared constraints
- LRA deviations, end-point and end-direction deviations, envelope (maximum centreline distance)
- ResultRecords per toleranced value
- `pack.evaluate_tube@1` (R1)

**Out** (and which node owns it instead):
- Best-fit solver (owned by `ALN`)
- Reconstruction and conversion (owned by `IND.TUBE.RECON` and `IND.TUBE.BEND`)
- Corrections (owned by `IND.TUBE.CORR`)

## Requirements

- **REQ-IND.TUBE.CMP-001** When measured and nominal tubes are compared, the pack shall report per-straight length deviation and per-bend rotation and bend-angle deviation.
- **REQ-IND.TUBE.CMP-002** The pack shall report end-point position deviation and end-direction angular deviation at both ends under the declared best-fit constraint (`ENDS_FIXED`, `START_FIXED` or `FREE`), and shall record the constraint and fitted transform in every ResultRecord.
- **REQ-IND.TUBE.CMP-003** The pack shall report the maximum distance between measured and nominal centrelines after the fit as an envelope value.
- **REQ-IND.TUBE.CMP-004** If the measured and nominal tubes have different numbers of bends, then the pack shall refuse the comparison with a typed error stating both counts.
- **REQ-IND.TUBE.CMP-005** Every toleranced value shall be emitted as a ResultRecord through `IND.CONTRACT` with UEC-0 unless an uncertainty evaluation is attached.

## Interfaces

- **Offers:** `pack.evaluate_tube@1` (R1); `TubeComparison` type used by CORR and TPL.
- **Consumes:** `IND.TUBE.MODEL`, `IND.TUBE.RECON`, `IND.TUBE.BEND`; ALN constrained best fit.

## Decomposition hints

FIT (constrained centreline fit), DEV (LRA and end deviations), ENV (envelope).

## Open questions

- How tube shops define "end direction" deviation (angle only, or angle split into two planes) [unverified].

## Children

_None yet: status `open`._
