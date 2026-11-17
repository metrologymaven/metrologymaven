---
id: LVM.RPT.SUMMARY
title: Network summary block and simulated marking
level: 3
parent: LVM.RPT
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-LVM.RPT-002, REQ-LVM.RPT-005]
covers: []
depends_on: [LVM.ADJ.STATS]
cli: [fid net report-summary]
mcp: [net_report_summary]
journal: []
updated: 2026-11-16
---

# LVM.RPT.SUMMARY: Network summary block and simulated marking

## Purpose

States, on one block, how an adjustment was made and how good it is: UNC model IDs, datum, weighting labels, variance factor and global test, degrees of freedom, counts and the `simulated` marking. It also owns the rule that a simulated network is marked on every report page.

## Scope

**In:**
- Summary block content: network label, adjustment time, algorithm ID and kernel version, datum definition, UNC model IDs and versions, user-declared weight labels, variance factor, global-test outcome, degrees of freedom, counts of stations, points, observations, constraints and flags
- `simulated` marking on every page and in every exported section

**Out** (and which node owns it instead):
- Tables (`LVM.RPT.TABLES`)
- Page layout (`RPT`)

## Requirements

- **REQ-LVM.RPT.SUMMARY-001** The summary block shall state the datum definition, the UNC model IDs and versions, the variance factor, the global-test outcome, the degrees of freedom and whether any weights were user-declared, copied from the adjustment result.
- **REQ-LVM.RPT.SUMMARY-002** If any station weights were user-declared, then the summary shall list those stations by label.
- **REQ-LVM.RPT.SUMMARY-003** If the network carries the `simulated` flag, then the system shall emit a `SIMULATED` marking that the report section contract requires on every page and every exported section.
- **REQ-LVM.RPT.SUMMARY-004** If the global test was not evaluated, then the summary shall say so instead of showing a pass or fail.

## Interfaces

- **Offers:** `net.report_summary@1` (R1); summary block and page-marking flag for `LVM.RPT.EXPORT`.
- **Consumes:** `LVM.ADJ.STATS` and `LVM.ADJ.SOLVE` result metadata.

## Decomposition hints

Likely a single leaf once the adjustment result metadata is fixed.

## Open questions

- Does the RPT template system already offer a per-page watermark contract that the `SIMULATED` marking can use? Needs RPT.MODEL input.

## Children

_None yet: status `open`._
