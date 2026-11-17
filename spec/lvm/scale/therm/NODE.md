---
id: LVM.SCALE.THERM
title: Thermal correction of reference lengths
level: 3
parent: LVM.SCALE
kind: feature
status: leaf
phase: P3
size: S
owners: []
claimed_by:
satisfies: [REQ-LVM.SCALE-002]
covers: []
depends_on: [LVM.SCALE.BIND]
side_effect: W2
determinism: D0
cli: [fid net set-scale-temperature]
mcp: [net_set_scale_temperature]
journal: [net.set_scale_temperature@1]
updated: 2026-11-16
---

# LVM.SCALE.THERM: Thermal correction of reference lengths

## Purpose

Corrects a reference length from its calibration temperature to the temperature at which it was measured, and estimates the standard uncertainty of that correction. The correction is stored on the binding, because one artefact can sit at different temperatures in different placements.

## Scope

**In:**
- `net.set_scale_temperature@1` on a binding: measurement temperature with optional standard uncertainty, and its source (manual entry or a recorded sensor reading ID)
- Linear thermal correction and its first-order standard uncertainty
- Clearing the correction

**Out** (and which node owns it instead):
- Temperature sensor drivers and readings (`HAL`)
- Using the corrected length as a constraint (`LVM.SCALE.BIND`)

## Requirements

- **REQ-LVM.SCALE.THERM-001** When `net.set_scale_temperature@1` runs with temperature T, the system shall compute L_T = L_ref · (1 + α · (T − T_ref)) in that evaluation order and store it on the binding together with T, its authored value and its source.
- **REQ-LVM.SCALE.THERM-002** The system shall compute the correction's standard uncertainty as u_th = √((L_ref · α · u_T)² + (L_ref · (T − T_ref) · u_α)²), treating a missing u_T or u_α as 0 and stating which inputs were missing.
- **REQ-LVM.SCALE.THERM-003** The journal event shall record T, u_T, α, u_α, T_ref, L_ref and the resulting L_T and u_th, so replay reproduces the stored values bit-exactly.
- **REQ-LVM.SCALE.THERM-004** If T is non-finite or outside 173.15 K to 373.15 K [unverified], then the command shall fail with `NET_SCALE_TEMP_INVALID` and journal nothing.
- **REQ-LVM.SCALE.THERM-005** When the command runs with `--clear`, the system shall remove the correction so the binding reverts to the calibrated length.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/lvm/net-scalebar-2pos` (SB1: L_ref = 1.5000213 m, T_ref = 293.15 K, α = 2.0e-6 /K), when `fid net set-scale-temperature --binding B1 --t "21.0 degC" --u-t "0.1 K" --json` runs, then L_T equals 1.5000213 · (1 + 2.0e-6 · 1.0) evaluated in f64 without FMA, and u_th equals 1.5000213 · 2.0e-6 · 0.1 within 1 ulp.
- **AC-2** (missing input) Given no `--u-t` and no u_α, then u_th = 0 and the result lists `u_T` and `u_alpha` as missing.
- **AC-3** (error path) Given `--t "500 degC"`, then the command fails with `NET_SCALE_TEMP_INVALID` and the journal length is unchanged.
- **AC-4** (clear) Given a correction set, when `--clear` runs, then `scale_constraints` again uses 1.5000213 m for B1.
- **AC-5** (replay) Given the journal from AC-1 to AC-4, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command:** `net.set_scale_temperature@1` (W2): `binding: BindingId, t: Option<Quantity>, u_t: Option<Quantity>, source: Manual|SensorReading(ReadingId), clear: bool` → `ThermalCorrection`.
- **Algorithm:** linear thermal expansion model; first-order uncertainty by the law of propagation of uncertainty (JCGM 100:2008, clause 5.1), uncorrelated inputs. Temperatures in kelvin; the difference T − T_ref is computed first.
- **Errors:** `NET_SCALE_TEMP_INVALID`, `NET_UNKNOWN_BINDING`.
- **Determinism:** D0 (only +, −, × and correctly rounded sqrt in fixed order; no FMA).
- **Crate:** `fid-lvm` (proposed name).

## Test fixtures

- `fixtures/lvm/net-scalebar-2pos` (shared with `LVM.SCALE.BIND`), CC-BY-4.0.

## Build prompt

> In the crate `fid-lvm`, add the W2 registry command `net.set_scale_temperature@1 (binding, t, u_t, source, clear)` with CLI `fid net set-scale-temperature` and `--json`; the MCP name is generated. It stores on a reference-length binding the corrected length L_T = L_ref · (1 + α · (T − T_ref)) and its standard uncertainty u_th = √((L_ref·α·u_T)² + (L_ref·(T − T_ref)·u_α)²), with missing u_T or u_α treated as 0 and listed as missing. Temperatures are converted to kelvin with one journaled rounding; reject non-finite T or T outside 173.15–373.15 K with `NET_SCALE_TEMP_INVALID` (journal nothing). `--clear` removes the correction. The journal event records every input and both outputs. Evaluate in the stated order with no FMA (KNP-1, CONSTITUTION §4); no panics, no `unwrap`, no `unsafe` (CONSTITUTION §8). Tests: the five acceptance criteria above. Determinism class D0.

## Open questions

- Should a temperature read from a live HAL sensor be captured automatically when a binding's end points are measured? That would be a later extension depending on `HAL.LIVE`.

## Children

_Leaf: none._
