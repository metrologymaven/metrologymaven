---
id: LVM.MOVE.PROP
title: Move pose uncertainty and far-end propagation
level: 3
parent: LVM.MOVE
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-LVM.MOVE-004]
covers: []
depends_on: [LVM.MOVE.SOLVE, UNC.PROP, UNC.XFM, UNC.INST]
cli: [fid net estimate-move-uncertainty]
mcp: [net_estimate_move_uncertainty]
journal: []
updated: 2026-11-16
---

# LVM.MOVE.PROP: Move pose uncertainty and far-end propagation

## Purpose

Before a move is accepted, the user should see what it costs in accuracy. This node estimates the covariance of the new station pose from the UNC instrument models of both stations and the chosen common-point geometry, and propagates it to the far end of the working volume, so the user can compare candidate point sets.

## Scope

**In:**
- Pose covariance of a solved move from both stations' instrument models
- Propagated position uncertainty at user-chosen evaluation points, by default the working-volume corner farthest from the to-station
- Comparison of two candidate point sets

**Out** (and which node owns it instead):
- Propagation engines and transform uncertainty types (`UNC.PROP`, `UNC.XFM`)
- Instrument models (`UNC.INST`)
- Chains of moves (`LVM.MOVE.CHAIN`)

## Requirements

- **REQ-LVM.MOVE.PROP-001** When `net.estimate_move_uncertainty@1` runs on a move solution, the system shall return the 6×6 pose covariance derived from the UNC models of both stations, with the model IDs and versions.
- **REQ-LVM.MOVE.PROP-002** The system shall report the propagated position uncertainty (standard and expanded with stated coverage factor) at the far end of the working volume and at any user-supplied evaluation points.
- **REQ-LVM.MOVE.PROP-003** If either station lacks a UNC model and declared weights, then the estimate shall return UEC-0 ("not evaluated") rather than a number.
- **REQ-LVM.MOVE.PROP-004** Given a synthetic move, a Monte Carlo propagation with 10 000 trials [unverified] shall agree with the linear estimate within 10 % [unverified] at the far-end point.

## Interfaces

- **Offers:** `net.estimate_move_uncertainty@1` (R1); `PoseCovariance` for `LVM.MOVE.CHAIN` and `LVM.MOVE.CHECK` display.
- **Consumes:** `LVM.MOVE.SOLVE`; `UNC.PROP`; `UNC.XFM`; `UNC.INST`.

## Decomposition hints

POSECOV (pose covariance), FAREND (evaluation-point propagation).

## Open questions

- Is linear propagation sufficient for typical move geometries, or should Monte Carlo be the default? Needs UNC input.

## Children

_None yet: status `open`._
