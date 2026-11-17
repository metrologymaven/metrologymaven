---
id: LVM.RPT.TABLES
title: Residual tables, flagged observations and ellipsoid data
level: 3
parent: LVM.RPT
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-LVM.RPT-001]
covers: [SA.NET.04]
depends_on: [LVM.ADJ.STATS, LVM.SCALE.CHECK]
cli: [fid net report-residuals]
mcp: [net_report_residuals]
journal: []
updated: 2026-11-16
---

# LVM.RPT.TABLES: Residual tables, flagged observations and ellipsoid data

## Purpose

Builds the tabular content of a network report from an adjustment result: residuals per observation, aggregates per station and per common point, the flagged-observation list, the scale-check table and the uncertainty-ellipsoid data per point. Values are copied unchanged from `LVM.ADJ.STATS` and `LVM.SCALE.CHECK`; this node only selects, orders and aggregates them.

## Scope

**In:**
- Per-observation table: station, common point, type, residual components, standardised residuals, redundancy number, flag status
- Per-station and per-point aggregates (count, RMS of residuals, maximum standardised residual)
- Flagged-observation list with reason
- Scale-check table rows
- Point ellipsoid data: semi-axes and orientation from each 3×3 adjusted covariance, at a stated coverage

**Out** (and which node owns it instead):
- Computing residual statistics (`LVM.ADJ.STATS`) and scale checks (`LVM.SCALE.CHECK`)
- Summary block (`LVM.RPT.SUMMARY`)
- Rendering ellipsoids (`VIZ`)

## Requirements

- **REQ-LVM.RPT.TABLES-001** The system shall produce one per-observation row for every observation in the adjustment, enabled or not, with station, common point, observation type, residual components, standardised residuals, redundancy number and flag status copied unchanged from the adjustment result.
- **REQ-LVM.RPT.TABLES-002** Rows shall be ordered by station creation order, then common-point creation order, then observation creation order.
- **REQ-LVM.RPT.TABLES-003** The system shall produce per-station and per-point aggregates whose RMS values are computed with fixed-order reductions.
- **REQ-LVM.RPT.TABLES-004** The system shall produce ellipsoid semi-axes and axis directions for each adjusted point from its 3×3 covariance at a stated coverage probability, and shall omit them when the covariance is UEC-0.
- **REQ-LVM.RPT.TABLES-005** Disabled observations shall appear in a separate section marked "excluded", with no residual values.

## Interfaces

- **Offers:** `net.report_residuals@1` (R1); table blocks for `LVM.RPT.EXPORT`; ellipsoid and residual-vector data for VIZ.
- **Consumes:** `LVM.ADJ.STATS` results; `LVM.SCALE.CHECK` rows.

## Decomposition hints

OBS (per-observation rows), AGG (aggregates), ELL (ellipsoid data).

## Open questions

- Which coverage probability should ellipsoids use by default (for example 95 % for a 3-D normal distribution) [unverified]?

## Children

_None yet: status `open`._
