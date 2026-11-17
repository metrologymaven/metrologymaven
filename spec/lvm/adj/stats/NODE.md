---
id: LVM.ADJ.STATS
title: Residual statistics, gross-error flags and UEC gating
level: 3
parent: LVM.ADJ
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-LVM.ADJ-006, REQ-LVM.ADJ-009]
covers: []
depends_on: [LVM.ADJ.SOLVE, UNC.UEC]
cli: [fid net set-flag-threshold]
mcp: [net_set_flag_threshold]
journal: [net.set_flag_threshold@1]
updated: 2026-11-16
---

# LVM.ADJ.STATS: Residual statistics, gross-error flags and UEC gating

## Purpose

Computes the a-posteriori statistics that let a user judge an adjustment: residuals and standardised residuals per observation, redundancy numbers, the variance factor and its global test, and flags on observations that look like gross errors. It also decides whether adjusted uncertainties may be attached to downstream ResultRecords and with which UEC class. Flags never change values; excluding an observation is always a separate journaled user command.

## Scope

**In:**
- Residuals, cofactor of residuals, standardised residuals and redundancy numbers per observation
- A-posteriori variance factor, degrees of freedom and a global test of the variance factor
- Gross-error flags against a user-set threshold
- UEC class and gating of adjusted uncertainties for downstream ResultRecords

**Out** (and which node owns it instead):
- Excluding observations (`LVM.STATION.OBS`, user command)
- Presentation of statistics (`LVM.RPT`)
- UEC definitions (`UNC.UEC`)

## Requirements

- **REQ-LVM.ADJ.STATS-001** For each enabled observation component, the system shall report the residual, its standard deviation from the cofactor matrix of residuals, the standardised residual and the redundancy number.
- **REQ-LVM.ADJ.STATS-002** The system shall report the a-posteriori variance factor, the degrees of freedom, and the outcome of a global test of the variance factor at a user-set significance level (default 5 %) [unverified].
- **REQ-LVM.ADJ.STATS-003** The system shall flag every observation whose absolute standardised residual exceeds the threshold set by `net.set_flag_threshold@1` (default 3) [unverified], and flagging shall never change any adjusted value.
- **REQ-LVM.ADJ.STATS-004** The sum of redundancy numbers shall equal the degrees of freedom within 1e-9 on the fixture networks.
- **REQ-LVM.ADJ.STATS-005** The system shall attach adjusted uncertainties to downstream ResultRecords only when the global test was evaluated, with the UEC class assigned by UNC; otherwise it shall attach UEC-0.

## Interfaces

- **Offers:** `AdjustmentStats` for `LVM.RPT`; `net.set_flag_threshold@1` (W2); UEC-gated uncertainty for ALN and GDT consumers.
- **Consumes:** `LVM.ADJ.SOLVE` results; `UNC.UEC` classes.

## Decomposition hints

RESID (residual statistics), GLOBAL (variance-factor test), FLAG (gross-error flags), UEC (gating).

## Open questions

- Which gross-error test (data snooping, tau test or another) and which thresholds should be the default? Needs a geodesy or large-volume metrology reviewer.
- Robust estimators (iteratively reweighted) would change values of record; if offered, they must be proposals that a human adopts (C5). Not in scope until decided.
- If the global test fails, should adjusted uncertainties be scaled by the variance factor, or left unscaled with a warning? Needs UNC input.

## Children

_None yet: status `open`._
