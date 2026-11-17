---
id: LVM.MOVE
title: Station moves (leapfrog)
level: 2
parent: LVM
kind: subsystem
status: split
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-LVM-003]
covers: [SA.NET.02]
depends_on: [LVM.STATION, ALN, UNC]
cli: [fid net *-move]
mcp: [net_*_move]
journal: [net.*_move@1]
updated: 2026-11-16
---

# LVM.MOVE: Station moves (leapfrog)

## Purpose

Moving an instrument to a new position and tying it back to the existing network through common points. This node owns the move workflow: picking the common points, the quick pose solve for the new station, and the geometry checks that warn before a weak move is accepted. The result is an initial station pose that `LVM.ADJ` later refines in the full network.

## Scope

**In:**
- Move session: from-station, to-station, chosen common points
- Quick pose solve for the new station from common points (via ALN point-pair transforms)
- Pre-acceptance geometry checks: point count, spatial spread, conditioning, residuals
- Warnings and refusal thresholds, with explicit user override that is journaled
- Move history and chaining (move of a move)

**Out** (and which node owns it instead):
- Full network adjustment (`LVM.ADJ`)
- Observation storage (`LVM.STATION`)
- Device-level leapfrog bookkeeping and instrument re-initialisation (`HAL`)
- Point-pair transform solver (`ALN`)

## Requirements

- **REQ-LVM.MOVE-001** When a move is solved, the system shall compute the new station pose from at least 3 non-collinear common points and report per-point residuals and RMS.
- **REQ-LVM.MOVE-002** If fewer than 4 common points are used, or the common points span less than a user-set fraction of the working volume (default 25 %) [unverified], then the system shall warn that the move geometry is weak.
- **REQ-LVM.MOVE-003** If the condition number of the move solve exceeds a configured limit, then the system shall refuse to accept the move unless the user overrides it with a journaled reason.
- **REQ-LVM.MOVE-004** The system shall estimate the uncertainty of the new station pose from the UNC models of both stations and show the expected propagated uncertainty at the far end of the working volume before the move is accepted.
- **REQ-LVM.MOVE-005** When a chain of moves is built, the system shall report the accumulated pose uncertainty along the chain.

## Interfaces

- **Offers:** `net.plan_move@1` (R1), `net.solve_move@1` (R1), `net.accept_move@1` (W2); initial station poses for `LVM.ADJ`.
- **Consumes:** `LVM.STATION` observations; ALN point-pair transforms; UNC models and propagation.

## Decomposition hints

PLAN (common-point choice and coverage), SOLVE (quick pose solve), CHECK (geometry quality and thresholds), CHAIN (accumulated uncertainty).

## Split rationale

The dividing principle is **the order of a move**: solve a candidate pose (SOLVE), see what it costs in uncertainty (PROP), judge and accept it (CHECK), then track how moves accumulate (CHAIN). Planning (common-point choice) is folded into SOLVE because the plan is just its input list.

Alternatives considered:
- **Separate PLAN child.** Rejected: listing shared common points is a small query with no rules of its own.
- **Merge PROP into CHECK.** Rejected: PROP depends on UNC propagation and models, which CHECK's count, spread and conditioning rules do not need, so CHECK can ship first.

## Open questions

- Default thresholds for "weak geometry" need a large-volume metrology reviewer; the 25 % spread figure is a placeholder. Carried in `LVM.MOVE.CHECK`.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| LVM.MOVE.SOLVE | Move sessions and quick pose solve | 001 | SA.NET.02 |
| LVM.MOVE.CHECK | Move geometry checks and acceptance | 002 003 | none |
| LVM.MOVE.PROP | Move pose uncertainty and far-end propagation | 004 | none |
| LVM.MOVE.CHAIN | Move history and accumulated chain uncertainty | 005 | none |
