---
id: UNC.INST.PORT
title: Per-observation models for trackers, arms and scanners
level: 3
parent: UNC.INST
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.INST-002, REQ-UNC.INST-005]
covers: [SA.UNC.01]
depends_on: [UNC.INST.SCHEMA, UNC.MODEL.DIST]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.INST.PORT: Per-observation models for trackers, arms and scanners

## Purpose

Turns a single observation from a portable or optical instrument into a 3x3 covariance in SI units, using the bound model's parameters: laser trackers (range, horizontal and vertical angle noise, plus range-dependent terms), articulated arms (probing noise and pose-dependent terms) and scanners (range, incidence angle and point-noise terms). These covariances weight network adjustment and transforms and feed Monte Carlo fields.

## Scope

**In:**
- Observation model kinds: spherical (tracker, total station), arm pose, optical point (scanner)
- Covariance from parameters: angle and range terms rotated into Cartesian coordinates, incidence-angle dependence for scanners
- Evaluation entry point `observation_covariance(model, observation) -> Cov3`

**Out:**
- Weight export (owned by `UNC.INST.WEIGHT`)
- Network adjustment (owned by `LVM`)
- Spatial fields (owned by `UNC.XFM.FIELD`)

## Requirements

- **REQ-UNC.INST.PORT-001** When an observation from a tracker, arm or scanner is evaluated with a bound observation model, the system shall return its 3x3 covariance in SI units in the instrument frame.
- **REQ-UNC.INST.PORT-002** For spherical observations, the covariance shall be the Jacobian of the spherical-to-Cartesian map times the diagonal (or declared) covariance of range and angles, computed in the kernel.
- **REQ-UNC.INST.PORT-003** For scanner points, the covariance shall depend on range and incidence angle as declared in the model, and points beyond the model's incidence limit shall be flagged `out_of_model_range`.
- **REQ-UNC.INST.PORT-004** The returned covariance shall be symmetric positive semi-definite, and if the model parameters would produce an indefinite matrix, then evaluation shall fail with `invalid_covariance`.
- **REQ-UNC.INST.PORT-005** Evaluating 10^6 tracker observations shall complete in under 1 s on reference hardware [unverified].

## Interfaces

- **Offers:** `observation_covariance`; observation model kinds in the `instrument-model/v1` format.
- **Consumes:** model format and range check (UNC.INST.SCHEMA); distributions (UNC.MODEL.DIST).

## Decomposition hints

SPH (spherical instruments), ARM (arm pose models), OPT (scanner point models).

## Open questions

- Split: the three model kinds are pure kernel functions in `fid-kunc` (UNC math, layer L0) that take typed parameters; EVAL in `fid-unc-inst` adapts a bound `instrument-model/v1` to them, applies the range check and the PSD check, and offers the batch entry point. Kernel children therefore do not depend on `UNC.INST.SCHEMA`.

- Arm pose-dependent models need joint-angle data from HAL; whether drivers expose them is unconfirmed.
- Default parameter values for example models must come from public sources only [unverified].

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| UNC.INST.PORT.EVAL | Observation covariance entry point, dispatch, checks and batch (leaf) | 001 004 005 | SA.UNC.01 |
| UNC.INST.PORT.SPH | Spherical observation covariance for trackers and total stations (leaf) | 001 002 | SA.UNC.01 |
| UNC.INST.PORT.ARM | Articulated-arm point covariance without joint data (leaf) | 001 | SA.UNC.01 |
| UNC.INST.PORT.OPT | Scanner point covariance from range and incidence (leaf) | 001 003 | SA.UNC.01 |
