---
id: CAL.ASSET
title: Calibration asset lineage, intervals and out-of-tolerance impact
level: 2
parent: CAL
kind: subsystem
status: open
phase: P4
size: L
owners: []
claimed_by:
satisfies: [REQ-CAL-005]
covers: []
depends_on: [UNC.INST, RPT.CATALOG, CORE.JRN]
cli: [fid cal trace-impact, fid cal schedule]
mcp: [cal_trace_impact, cal_schedule]
journal: [cal.trace_impact, cal.schedule]
updated: 2026-11-22
---

# CAL.ASSET: Calibration asset lineage, intervals and out-of-tolerance impact

## Purpose

Connects calibration certificates and master artifacts to `UNC.INST.LIB` and `RPT.CATALOG`: tracks traceability hierarchies (master → working gauge → part results), calibration interval drift analysis (NCSL RP-1), and reverse out-of-tolerance (OOT) impact queries identifying every part ResultRecord measured with an instrument later found out of tolerance.

## Scope

**In:** Master-to-instrument lineage graph, calibration due/overdue schedules, historical drift series and interval review diagnostics, and reverse OOT impact queries across `RPT.CATALOG`.
**Out:** The core instrument/gauge register itself (`UNC.INST.LIB`); certificate issuance (`CAL.CERT`).

## Requirements

- **REQ-CAL.ASSET-001** Given an out-of-tolerance calibration record for a gauge or instrument, the system shall deterministically enumerate all ResultRecords in `RPT.CATALOG` captured since the prior conforming calibration.
- **REQ-CAL.ASSET-002** Master-artifact traceability links shall be content-addressed by certificate digest so historical provenance resolves even after an asset is retired.

## Children

_None yet: status `open`._
