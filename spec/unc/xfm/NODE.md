---
id: UNC.XFM
title: Transform uncertainty and spatial uncertainty fields
level: 2
parent: UNC
kind: subsystem
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC-002, REQ-UNC-003]
covers: [SA.UNC.02, SA.UNC.03]
depends_on: [UNC.PROP, UNC.INST, ALN]
cli: [fid unc *-transform-uncertainty, fid unc *-field]
mcp: [unc_*_transform_uncertainty, unc_*_field]
journal: [unc.*_transform_uncertainty, unc.*_field]
updated: 2026-11-16
---

# UNC.XFM: Transform uncertainty and spatial uncertainty fields

## Purpose

Quantifies the uncertainty of **coordinate transforms** (alignments, frames, station poses) and carries it into the points and features expressed in the transformed frame. It also produces **spatial uncertainty fields**: per-point or per-feature covariance over a measurement volume or network, computed by Monte Carlo through the instrument models (the SpatialAnalyzer-style "uncertainty field"). It is separate from `UNC.PROP` because it deals with rigid-body and similarity transforms on the manifold and with spatially indexed outputs, which networks and alignment both consume.

## Scope

**In:**
- 6-DoF and 7-DoF (with scale) transform covariance from alignment and point-pair fits
- Propagating transform covariance into transformed points, features and characteristics
- Chained transforms: accumulating covariance along alignment hierarchies
- Monte Carlo uncertainty fields over a set of points or a network
- Field outputs for display (ellipsoids) as data, not rendering

**Out** (and which node owns it instead):
- The alignment solvers themselves (owned by `ALN`)
- Network and bundle adjustment (owned by `LVM`)
- Instrument models and weights (owned by `UNC.INST`)
- Generic LPU and MCM engines (owned by `UNC.PROP`)
- Drawing ellipsoids (owned by `VIZ`)

## Requirements

- **REQ-UNC.XFM-001** When an alignment or point-pair transform is computed, the system shall be able to produce its 6x6 parameter covariance (7x7 with scale) in a declared parameterisation.
- **REQ-UNC.XFM-002** When a point or feature is expressed in a frame whose transform has a covariance, the system shall include the transform contribution in that point's or feature's covariance.
- **REQ-UNC.XFM-003** When transforms are chained, the system shall propagate covariance through the chain and keep the correlation between transforms that share input observations.
- **REQ-UNC.XFM-004** The system shall compute a Monte Carlo uncertainty field that gives a 3x3 covariance for each requested point using the bound instrument models, with determinism class D2.
- **REQ-UNC.XFM-005** The transform covariance from LPU shall agree with an MCM evaluation of the same transform within 10 % on every standard deviation for the ORS alignment cases [unverified].
- **REQ-UNC.XFM-006** If an input observation set has no bound instrument model, then the system shall mark the transform uncertainty UEC-0 and shall not report a covariance.

## Interfaces

- **Offers:** `TransformCovariance` and `UncertaintyField` types; commands such as `unc.compute_transform_uncertainty@1` (R1), `unc.compute_field@1` (R1), commit to the document as W2 (journal `unc.*_field`). Consumed by `LVM`, `ALN` reports and `VIZ`.
- **Consumes:** propagation engines from `UNC.PROP`; instrument covariance and weights from `UNC.INST`; transforms and their input observations from `ALN`.

## Decomposition hints

TCOV (transform covariance), APPLY (propagation into transformed entities), CHAIN (hierarchies), FIELD (Monte Carlo fields).

## Open questions

- The parameterisation of rotation covariance (Lie algebra vs Euler angles) is a design choice for a leaf, but reports must state it.

## Children

_None yet: status `open`._
