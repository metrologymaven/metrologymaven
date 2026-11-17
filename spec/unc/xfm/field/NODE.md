---
id: UNC.XFM.FIELD
title: Monte Carlo spatial uncertainty fields
level: 3
parent: UNC.XFM
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.XFM-004, REQ-UNC.XFM-006]
covers: [SA.UNC.02]
depends_on: [UNC.PROP.MCM, UNC.INST.PORT, UNC.XFM.APPLY]
cli: [fid unc compute-field, fid unc commit-field]
mcp: [unc_compute_field, unc_commit_field]
journal: [unc.commit_field@1]
updated: 2026-11-16
---

# UNC.XFM.FIELD: Monte Carlo spatial uncertainty fields

## Purpose

Computes an uncertainty field: a 3x3 covariance for each requested point over a measurement volume or instrument network, obtained by Monte Carlo through the bound instrument observation models and the transforms that place each instrument. The field is data (covariances and derived ellipsoid axes) that `VIZ` draws and `LVM` and `UNC.PLAN` consume.

## Scope

**In:**
- `UncertaintyField { points, cov: Vec<Cov3>, frame, trials, seed, model refs }`
- Monte Carlo over instrument observations and station transforms with seeded trials (D2)
- Derived ellipsoid axes and lengths per point at a stated coverage
- `unc.compute_field@1` (R1) and `unc.commit_field@1` (W2)
- UEC-0 for points whose observations lack a bound model

**Out:**
- Rendering ellipsoids (owned by `VIZ`)
- Network adjustment (owned by `LVM`)
- Predictive measurability before measurement (owned by `UNC.PLAN`)

## Requirements

- **REQ-UNC.XFM.FIELD-001** When a field is requested for a set of points, the system shall return a 3x3 covariance per point from Monte Carlo through the bound instrument models and station transforms, with determinism class D2.
- **REQ-UNC.XFM.FIELD-002** If a point's observations have no bound instrument model, then that point shall be UEC-0 in the field and carry no covariance.
- **REQ-UNC.XFM.FIELD-003** The field shall state its frame, trial count, seed, and the digests of every model and transform used.
- **REQ-UNC.XFM.FIELD-004** A field for 10,000 points from 4 stations with 10^4 trials shall complete in under 30 s on reference hardware [unverified].

## Interfaces

- **Offers:** `UncertaintyField`; `unc.compute_field@1`, `unc.commit_field@1`; ellipsoid data for VIZ.
- **Consumes:** MCM engine (UNC.PROP.MCM); observation covariance (UNC.INST.PORT); transform application (UNC.XFM.APPLY).

## Decomposition hints

SIM (trial generation over stations and observations), STATS (per-point covariance and ellipsoids), CMD (compute and commit).

## Open questions

- Fields matter most for large-volume networks (LVM, P3); this child is tagged P1b with its parent, and reviewers may move it to P3 by decision.

## Children

_None yet: status `open`._
