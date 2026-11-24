---
id: CAL.MACH
title: ISO 230 machine-tool geometric verification and ballbar diagnostics
level: 2
parent: CAL
kind: subsystem
status: open
phase: P4
size: XL
owners: []
claimed_by:
satisfies: [REQ-CAL-002]
covers: []
depends_on: [KRN.NUM, KRN.LSQ, UNC.PROP]
cli: [fid cal verify-machine]
mcp: [cal_verify_machine]
journal: [cal.verify_machine]
updated: 2026-11-22
---

# CAL.MACH: ISO 230 machine-tool geometric verification and ballbar diagnostics

## Purpose

Deterministic ISO 230 geometric metrology for CNC machine tools: linear positioning accuracy and repeatability (ISO 230-2), circular ballbar diagnostics (ISO 230-4), diagonal displacement tests (ISO 230-6), spindle error analysis (ISO 230-7), and 21-parameter volumetric error models.

## Scope

**In:** Bidirectional linear positioning (`A`, `R`, `B`, `M`), angular pitch/yaw/roll and straightness/squareness reduction, telescoping ballbar circularity fault decomposition (backlash, reversal spikes, cyclic error, servo mismatch, squareness, scaling), and 21-parameter rigid-body kinematic error table export.
**Out:** Live CNC controller write-back (`ECO.SDK` / OMV plugins per D-054); CMM ISO 10360 acceptance (`CAL.CMM`).

## Requirements

- **REQ-CAL.MACH-001** The subsystem shall evaluate ISO 230-2 linear positioning and ISO 230-4 circular ballbar datasets deterministically under KNP-1, decomposing geometric error components with stated uncertainty.
- **REQ-CAL.MACH-002** The subsystem shall assemble 21-parameter volumetric error models with explicit axis-chain conventions and verifiable input digests.

## Children

_None yet: status `open`._
