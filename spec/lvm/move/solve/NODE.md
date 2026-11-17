---
id: LVM.MOVE.SOLVE
title: Move sessions and quick pose solve
level: 3
parent: LVM.MOVE
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-LVM.MOVE-001]
covers: [SA.NET.02]
depends_on: [LVM.STATION.OBS, ALN.XFM]
cli: [fid net plan-move, fid net solve-move]
mcp: [net_plan_move, net_solve_move]
journal: []
updated: 2026-11-16
---

# LVM.MOVE.SOLVE: Move sessions and quick pose solve

## Purpose

The core of a station move: the user names the from-station (already posed) and the to-station (newly set up), chooses the common points both have observed, and gets a quick pose for the new station with per-point residuals. The solve is a scratch computation; nothing is written until `LVM.MOVE.CHECK` accepts the move.

## Scope

**In:**
- `net.plan_move@1`: list of common points observed from both stations, with their coverage
- `net.solve_move@1`: converting each common point's observations to coordinates in both station frames, then a point-pair rigid transform through `ALN.XFM`
- Per-point residuals, RMS, and a non-collinearity test of the chosen points
- Using only enabled observations and points

**Out** (and which node owns it instead):
- Geometry warnings, refusal and acceptance (`LVM.MOVE.CHECK`)
- Pose uncertainty (`LVM.MOVE.PROP`)
- The point-pair transform solver (`ALN.XFM`)
- Full network refinement (`LVM.ADJ.SOLVE`)

## Requirements

- **REQ-LVM.MOVE.SOLVE-001** When `net.plan_move@1` runs, the system shall list every enabled common point observed from both stations, with the number of observations from each.
- **REQ-LVM.MOVE.SOLVE-002** When `net.solve_move@1` runs with at least 3 chosen common points, the system shall compute the to-station pose in the network frame by a rigid point-pair transform and report per-point residual vectors, their norms and the RMS.
- **REQ-LVM.MOVE.SOLVE-003** If fewer than 3 points are chosen, or the chosen points are collinear (ratio of second to first singular value of the centred point matrix below 1e-6) [unverified], then the solve shall fail with `NET_MOVE_DEGENERATE`.
- **REQ-LVM.MOVE.SOLVE-004** If a common point has a direction-only observation and no distance from either station, then the solve shall exclude it and report it as unusable for a quick solve.
- **REQ-LVM.MOVE.SOLVE-005** Given a noise-free synthetic move, the solved pose shall match truth within 1e-10 m and 1e-12 rad [unverified].
- **REQ-LVM.MOVE.SOLVE-006** The solve commands shall be R1: they shall return a scratch result and journal nothing.

## Interfaces

- **Offers:** `net.plan_move@1` (R1), `net.solve_move@1` (R1) returning `MoveSolution { pose, residuals, rms, condition, points }` for `LVM.MOVE.CHECK`.
- **Consumes:** `LVM.STATION.OBS` observations; `ALN.XFM` point-pair rigid transform.

## Decomposition hints

PLAN (common-point listing and coverage), POSE (conversion and point-pair solve).

## Open questions

- Should repeated observations of one point from one station be averaged before the solve, or each used as a separate pair?

## Children

_None yet: status `open`._
