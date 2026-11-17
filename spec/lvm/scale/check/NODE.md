---
id: LVM.SCALE.CHECK
title: Post-adjustment scale check
level: 3
parent: LVM.SCALE
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-LVM.SCALE-004]
covers: []
depends_on: [LVM.SCALE.BIND, LVM.ADJ.SOLVE]
cli: [fid net check-scale]
mcp: [net_check_scale]
journal: []
updated: 2026-11-16
---

# LVM.SCALE.CHECK: Post-adjustment scale check

## Purpose

After an adjustment, shows how well each reference length is reproduced: the adjusted length between its two points, the deviation from the calibrated (or thermally corrected) length, and that deviation relative to the combined uncertainty. Check-mode bindings are the independent test of scale; constraint-mode bindings show how strongly the network had to bend to fit them.

## Scope

**In:**
- Adjusted length and its standard uncertainty from the adjusted coordinates and their covariance
- Deviation and normalised deviation for every enabled binding, both modes
- Scale-check rows for `LVM.RPT`

**Out** (and which node owns it instead):
- Adjustment and covariance (`LVM.ADJ.SOLVE`)
- Presentation (`LVM.RPT`)

## Requirements

- **REQ-LVM.SCALE.CHECK-001** When `net.check_scale@1` runs on an adjustment result, the system shall report for each enabled binding the adjusted length, its standard uncertainty propagated from the 6×6 covariance of the two adjusted points, the reference length used and the deviation.
- **REQ-LVM.SCALE.CHECK-002** The system shall report the ratio of the absolute deviation to the combined standard uncertainty √(u_adj² + u_ref²) and flag ratios above a user-set limit (default 2) [unverified].
- **REQ-LVM.SCALE.CHECK-003** The report shall mark each row as `constraint` or `check`, and shall state that constraint rows are not independent tests.
- **REQ-LVM.SCALE.CHECK-004** If the adjustment result has no evaluated covariance (UEC-0), then the uncertainty and ratio columns shall show "not evaluated" and no flag shall be raised.

## Interfaces

- **Offers:** `net.check_scale@1` (R1); `ScaleCheckRow` list for `LVM.RPT`.
- **Consumes:** `LVM.ADJ.SOLVE` adjusted coordinates and covariance; `LVM.SCALE.BIND` bindings and reference values.

## Decomposition hints

Likely a single leaf once the `AdjustmentResult` type of `LVM.ADJ.SOLVE` is fixed.

## Open questions

- For a constraint-mode binding, should u_adj be computed from the full adjustment (including the constraint itself) or by leaving that constraint out? The leave-one-out form is a fairer test but costs one extra solve per binding.

## Children

_None yet: status `open`._
