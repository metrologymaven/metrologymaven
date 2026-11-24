---
id: CAL.CMM
title: ISO 10360 and ASME B89.4 coordinate-instrument verification
level: 2
parent: CAL
kind: subsystem
status: open
phase: P4
size: XL
owners: []
claimed_by:
satisfies: [REQ-CAL-001]
covers: []
depends_on: [KRN.LSQ, UNC.INST, UNC.DEC, HAL.ACQ]
cli: [fid cal verify-cmm]
mcp: [cal_verify_cmm]
journal: [cal.verify_cmm]
updated: 2026-11-22
---

# CAL.CMM: ISO 10360 and ASME B89.4 coordinate-instrument verification

## Purpose

Deterministic evaluation of coordinate measuring machine, portable arm, laser tracker and optical CMM acceptance and reverification tests under the ISO 10360 series and ASME B89.4, linking calibrated artifact values and thermal readings to pass/fail verdicts via `UNC.DEC`.

## Scope

**In:** ISO 10360-2 (`E_0`, `E_150`, `R_0`), 10360-5 (`P_FTU`, `P_Form`, `P_Size`), 10360-4 (`THP`), 10360-8/13 (optical/imaging), 10360-10 (laser trackers), 10360-12 (articulated arms `SPAT`), step-gauge/gauge-block/ball-bar/hole-plate artifact definitions and CTE thermal correction.
**Out:** Part-inspection MPE consumption (`UNC.INST.MPE`); machine-tool geometric error mapping (`CAL.MACH`).

## Requirements

- **REQ-CAL.CMM-001** The subsystem shall compute ISO 10360 length, probing, scanning and multi-stylus errors from measured artifact datasets with explicit thermal compensation and artifact uncertainty propagation.
- **REQ-CAL.CMM-002** Every verification run shall record artifact serial numbers, calibration certificate digests and decision-rule verdicts (`CONFORMS | NONCONFORMS | INDETERMINATE`) with standard clause citations.

## Children

_None yet: status `open`._
