---
id: LVM.RPT
title: Network residual and quality reports
level: 2
parent: LVM
kind: subsystem
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-LVM-002]
covers: [SA.NET.04]
depends_on: [LVM.ADJ, LVM.SCALE, RPT, VIZ]
cli: [fid net report-*, fid net compare-*]
mcp: [net_report_*, net_compare_*]
journal: [net.report_*@1, net.compare_*@1]
updated: 2026-11-16
---

# LVM.RPT: Network residual and quality reports

## Purpose

Makes an adjusted network reviewable. It turns the statistics that `LVM.ADJ` and `LVM.SCALE` compute into tables, plots and report sections: residuals per observation, per station and per common point, flagged observations, the variance factor, scale checks and point uncertainty ellipsoids. It also compares two adjustments of the same network (for example before and after excluding an observation) so the user can see what changed.

## Scope

**In:**
- Network report content: summary, per-station, per-point, per-observation residual tables
- Flagged-observation list with the flag reason
- Scale-check table
- Point uncertainty ellipsoid data for display
- Comparison of two adjustment results of the same network
- Report sections for the RPT template system

**Out** (and which node owns it instead):
- Computing residuals and statistics (`LVM.ADJ`, `LVM.SCALE`)
- Report template engine and layout (`RPT`)
- Rendering ellipsoids and residual vectors (`VIZ`)

## Requirements

- **REQ-LVM.RPT-001** The system shall produce a network report listing, for each observation, the station, common point, observation type, residual components, standardised residual and flag status, taken unchanged from the adjustment result.
- **REQ-LVM.RPT-002** The network report shall state the adjustment's UNC model IDs, datum definition, variance factor, degrees of freedom and whether any weights were user-declared.
- **REQ-LVM.RPT-003** When two adjustment results of the same network are compared, the system shall list per point the change in adjusted coordinates and in uncertainty, and the observations enabled in one but not the other.
- **REQ-LVM.RPT-004** Regenerating a network report from the same journal shall give identical content.
- **REQ-LVM.RPT-005** If the network carries the `simulated` flag, then every page of the network report shall state it.

## Interfaces

- **Offers:** `net.report_network@1` (R1), `net.compare_adjustments@1` (R1); network report sections for RPT templates; ellipsoid and residual-vector data for VIZ.
- **Consumes:** `LVM.ADJ` results; `LVM.SCALE` checks; RPT template engine; VIZ display.

## Decomposition hints

TABLES (residual tables), SUMMARY (network summary block), COMPARE (adjustment comparison), EXPORT (report sections and CSV export through IO).

## Open questions

- None yet.

## Children

_None yet: status `open`._
