---
id: LVM.SCALE
title: Scale bars and reference lengths
level: 2
parent: LVM
kind: subsystem
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-LVM-004]
covers: [SA.NET.05]
depends_on: [LVM.STATION, UNC]
cli: [fid net *-scale-bar, fid net *-reference-length]
mcp: [net_*_scale_bar, net_*_reference_length]
journal: [net.*_scale_bar@1, net.*_reference_length@1]
updated: 2026-11-16
---

# LVM.SCALE: Scale bars and reference lengths

## Purpose

Scale bars and other calibrated reference lengths fix the scale of a network, which matters most for photogrammetry and angle-only instruments. This node owns the definition of a reference length (calibrated value, calibration uncertainty, temperature behaviour, certificate reference), its binding to observed end points, and the constraint it contributes to the adjustment. It also reports how well each reference length is reproduced after adjustment.

## Scope

**In:**
- Reference-length artefacts: scale bars, calibrated distances between targets
- Calibrated value with unit, calibration uncertainty, reference temperature, coefficient of thermal expansion, certificate ID
- Thermal correction of the calibrated length to measurement temperature
- Binding a reference length to two common points
- The scale constraint supplied to `LVM.ADJ` (with weight from calibration uncertainty)
- Post-adjustment scale check: adjusted length vs calibrated length

**Out** (and which node owns it instead):
- Solving the network (`LVM.ADJ`)
- Common-point storage (`LVM.STATION`)
- Temperature sensor drivers (`HAL`)
- Calibration-certificate signing (`TRUST`)

## Requirements

- **REQ-LVM.SCALE-001** The system shall record each reference length with its calibrated value and unit, expanded calibration uncertainty with coverage factor, reference temperature, thermal expansion coefficient and certificate ID.
- **REQ-LVM.SCALE-002** When a measurement temperature is supplied, the system shall correct the calibrated length to that temperature and journal the correction as one rounding step.
- **REQ-LVM.SCALE-003** The system shall supply each bound reference length to the adjustment as a distance constraint between its two common points, weighted by its calibration uncertainty.
- **REQ-LVM.SCALE-004** After adjustment, the system shall report for each reference length the adjusted length, its deviation from the calibrated length and the ratio of that deviation to the combined uncertainty.
- **REQ-LVM.SCALE-005** If a reference length has no calibration uncertainty, then the system shall refuse to use it as a weighted constraint and shall allow it only as an unweighted check length.

## Interfaces

- **Offers:** `net.define_scale_bar@1`, `net.bind_reference_length@1` (W2); scale constraints for `LVM.ADJ`; scale check rows for `LVM.RPT`.
- **Consumes:** `LVM.STATION` common points; UNC for combining uncertainties; temperature readings via HAL.

## Decomposition hints

DEF (artefact definitions and certificates), THERM (thermal correction), CONSTR (constraint contribution), CHECK (post-adjustment scale check).

## Open questions

- Should scale bars observed in several orientations be modelled as one artefact with repeat observations, or as separate reference lengths?

## Children

_None yet: status `open`._
