---
id: UNC.PLAN.PRED
title: Expected uncertainty from simulated observations
level: 3
parent: UNC.PLAN
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.PLAN-003]
covers: [SA.UNC.04]
depends_on: [UNC.PLAN.VIS, UNC.INST.PORT, UNC.PROP, UNC.XFM]
cli: [fid unc predict-uncertainty]
mcp: [unc_predict_uncertainty]
journal: [unc.predict_uncertainty@1]
updated: 2026-11-16
---

# UNC.PLAN.PRED: Expected uncertainty from simulated observations

## Purpose

Predicts, before any measurement, the expanded uncertainty each planned characteristic would have: it simulates the observations each visible station would make from the bound instrument models, propagates them through the characteristic's uncertainty model and the station transforms, and reports an expected U. Predictions are a distinct type that can never be written into a ResultRecord.

## Scope

**In:**
- `Prediction { char_id, station_set, expected_U, k, method, model refs, seed }` type, distinct from `UncertaintyStatement`
- Simulated observations for visible (pose, target) pairs from `UNC.PLAN.VIS`
- Propagation through `UNC.PROP` (LPU where smooth, MCM otherwise) with station transform uncertainty from `UNC.XFM`
- `unc.predict_uncertainty@1` (R1)

**Out:**
- Visibility (owned by `UNC.PLAN.VIS`)
- Ranking and reports (owned by `UNC.PLAN.RANK` and `UNC.PLAN.REPORT`)

## Requirements

- **REQ-UNC.PLAN.PRED-001** When a planned characteristic has a bound uncertainty model and bound instrument models for its visible observations, the system shall report its expected expanded uncertainty with coverage factor and method.
- **REQ-UNC.PLAN.PRED-002** Predictions shall be a separate type labelled `prediction`, and the system shall reject any attempt to commit a prediction into a ResultRecord with `prediction_not_result`.
- **REQ-UNC.PLAN.PRED-003** If a characteristic has no visible observation in a station set, then its prediction shall be `not_measurable` rather than a numeric U.
- **REQ-UNC.PLAN.PRED-004** Monte Carlo predictions shall be seeded and deterministic class D2.

## Interfaces

- **Offers:** `Prediction`; `unc.predict_uncertainty@1`.
- **Consumes:** visibility matrix (UNC.PLAN.VIS); observation covariance (UNC.INST.PORT); engines (UNC.PROP); transform covariance (UNC.XFM).

## Decomposition hints

SIM (simulated observations), PROP (propagation wiring and prediction type).

## Open questions

- How a planned characteristic maps to simulated observations for CMM probing (points per feature, strategy) needs `CMM` strategy data; coordinate with CMM.STRAT.

## Children

_None yet: status `open`._
