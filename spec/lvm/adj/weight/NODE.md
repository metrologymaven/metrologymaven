---
id: LVM.ADJ.WEIGHT
title: Observation weighting from UNC instrument models
level: 3
parent: LVM.ADJ
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-LVM.ADJ-002, REQ-LVM.ADJ-003]
covers: [SA.NET.03]
depends_on: [UNC.INST, LVM.STATION.OBS]
cli: [fid net set-weighting, fid net declare-weights]
mcp: [net_set_weighting, net_declare_weights]
journal: [net.set_weighting@1, net.declare_weights@1]
updated: 2026-11-16
---

# LVM.ADJ.WEIGHT: Observation weighting from UNC instrument models

## Purpose

Turns each observation into a covariance block for the adjustment, using the UNC instrument model of its station, so that a tracker distance, an arm point and a total-station angle each count according to its stated uncertainty. It owns the weighting mode per station, the user-declared fallback when no model exists, and the record of which model and version produced each weight.

## Scope

**In:**
- Per-observation covariance from the station's UNC instrument model, evaluated at the observation (for example range-dependent distance uncertainty)
- Weighting modes per station: `model`, `instrument_supplied` (Cartesian covariance as stored), `declared`
- User-declared standard deviations per observation component as a labelled fallback
- Covariance for distance constraints from `LVM.SCALE`
- Recording model IDs and versions in the adjustment input digest

**Out** (and which node owns it instead):
- The instrument models themselves (`UNC.INST`)
- Using the covariance in the solve (`LVM.ADJ.SOLVE`)
- Calibration uncertainty of reference lengths (`LVM.SCALE.BIND`)

## Requirements

- **REQ-LVM.ADJ.WEIGHT-001** When a station's weighting mode is `model`, the system shall compute each observation's covariance block from the station's UNC instrument model evaluated at that observation, and record the model ID and version.
- **REQ-LVM.ADJ.WEIGHT-002** If a station has no UNC instrument model and no declared weights, then the adjustment shall refuse to run with `NET_WEIGHT_MISSING` naming the station.
- **REQ-LVM.ADJ.WEIGHT-003** When `net.declare_weights@1` sets standard deviations for a station, the system shall use them only for that station and label every result that depends on them as `user_declared`.
- **REQ-LVM.ADJ.WEIGHT-004** When a station's weighting mode is `instrument_supplied`, the system shall use the stored Cartesian covariance and record that no UNC model was applied.
- **REQ-LVM.ADJ.WEIGHT-005** If a computed covariance block is not symmetric positive definite, then the system shall stop with `NET_WEIGHT_NOT_PD` naming the observation.
- **REQ-LVM.ADJ.WEIGHT-006** Observations within one station shall be treated as uncorrelated unless the UNC model supplies correlations, and the result shall state which case applied.

## Interfaces

- **Offers:** `net.set_weighting@1` (W2), `net.declare_weights@1` (W2); `fn covariance(obs, station, mode) -> Cov` for `LVM.ADJ.SOLVE`.
- **Consumes:** `UNC.INST` instrument models (evaluation API, model ID and version); `LVM.STATION.OBS` observations; `LVM.SCALE.BIND` constraint uncertainty.

## Decomposition hints

MODE (weighting mode commands and labels), EVAL (UNC model evaluation per observation type), DECL (declared-weight fallback).

## Open questions

- UNC.INST has not yet fixed its model evaluation interface for polar instruments (angle and range terms, temperature dependence).
- Should correlations between observations of one station (for example a common scale error) be modelled in P3, or deferred?

## Children

_None yet: status `open`._
