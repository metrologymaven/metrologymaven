---
id: LVM.MOVE.CHECK
title: Move geometry checks and acceptance
level: 3
parent: LVM.MOVE
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-LVM.MOVE-002, REQ-LVM.MOVE-003]
covers: []
depends_on: [LVM.MOVE.SOLVE, LVM.STATION.MODEL]
cli: [fid net check-move, fid net accept-move, fid net set-move-thresholds]
mcp: [net_check_move, net_accept_move, net_set_move_thresholds]
journal: [net.accept_move@1, net.set_move_thresholds@1]
updated: 2026-11-16
---

# LVM.MOVE.CHECK: Move geometry checks and acceptance

## Purpose

Judges whether a solved move is geometrically sound before it is accepted, and accepts it. It applies the configured thresholds (point count, spatial spread, condition number, residuals), warns or refuses, and lets the user override a refusal only with a journaled reason. Acceptance writes the to-station's initial pose, which `LVM.ADJ` later refines.

## Scope

**In:**
- Checks: number of points, spread of points relative to the working volume, condition number of the solve, maximum residual and RMS
- Thresholds as journaled configuration with documented defaults
- Warning versus refusal levels, and override with a reason
- `net.accept_move@1` writing the initial pose and storing the check report

**Out** (and which node owns it instead):
- The solve (`LVM.MOVE.SOLVE`)
- Pose uncertainty shown before acceptance (`LVM.MOVE.PROP`)
- Move history (`LVM.MOVE.CHAIN`)

## Requirements

- **REQ-LVM.MOVE.CHECK-001** If fewer than 4 common points are used, the system shall return a `MOVE_WEAK_COUNT` warning.
- **REQ-LVM.MOVE.CHECK-002** If the diagonal of the common points' bounding box is less than a user-set fraction (default 25 %) [unverified] of the working-volume diagonal, then the system shall return a `MOVE_WEAK_SPREAD` warning; the working volume shall be the user-set box or, if none, the bounding box of all enabled network points.
- **REQ-LVM.MOVE.CHECK-003** If the condition number of the move solve exceeds the configured limit, then `net.accept_move@1` shall refuse with `MOVE_ILL_CONDITIONED` unless an override reason is given, and the reason shall be journaled with the acceptance.
- **REQ-LVM.MOVE.CHECK-004** When a move is accepted, the system shall write the to-station's initial-pose slot and store the full check report, including thresholds and any override, in the same journal event.
- **REQ-LVM.MOVE.CHECK-005** Changing thresholds shall be a journaled command `net.set_move_thresholds@1`, and every check report shall state the thresholds it used.

## Interfaces

- **Offers:** `net.check_move@1` (R1), `net.accept_move@1` (W2), `net.set_move_thresholds@1` (W2); accepted initial poses for `LVM.ADJ.DATUM`.
- **Consumes:** `LVM.MOVE.SOLVE` solutions; `LVM.STATION.MODEL` pose slot (move-acceptance capability).

## Decomposition hints

RULES (checks and thresholds), ACCEPT (acceptance, override and pose write).

## Open questions

- Default thresholds for "weak geometry" (spread fraction, condition-number limit, residual limit) need a large-volume metrology reviewer; 25 % is a placeholder.
- Should an agent be allowed to accept a move (W2 on an agent branch, adopted later by a human under C5) or should acceptance be human-only? This node assumes the default W2 rules.

## Children

_None yet: status `open`._
