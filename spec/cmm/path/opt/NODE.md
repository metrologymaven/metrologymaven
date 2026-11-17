---
id: CMM.PATH.OPT
title: Feature ordering optimisation
level: 3
parent: CMM.PATH
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-CMM.PATH-007, REQ-CMM.PATH-008]
covers: [PCD.PTH.02]
depends_on: [CMM.PATH.PLAN, CMM.COLL]
cli: [fid cmm optimize-path, fid cmm accept-path-optimization]
mcp: [cmm_optimize_path, cmm_accept_path_optimization]
journal: [cmm.optimize_path@1, cmm.accept_path_optimization@1]
updated: 2026-11-16
---

# CMM.PATH.OPT: Feature ordering optimisation

## Purpose

Reorders features (and the tip and angle groups they fall in) to shorten total transit length, subject to clearance and collision constraints and to steps the program marks as order-dependent (alignment blocks, evaluation dependencies, user-pinned order). The result is a proposal (R1) with before and after lengths; accepting it is a separate journaled command (W2).

## Scope

**In:**
- Cost model: transit length plus a configurable cost per tip change and head index
- Precedence constraints from order-dependent steps
- Seeded heuristic search (deterministic for a given seed)
- Proposal and acceptance commands

**Out** (and which node owns it instead):
- Plan compilation (owned by `CMM.PATH.PLAN`)
- Collision verdicts (owned by `CMM.COLL`)

## Requirements

- **REQ-CMM.PATH.OPT-001** When optimisation is requested, the system shall propose a feature order that does not increase total cost, never moves a step across an order-dependent boundary, and keeps every move `CLEAR`.
- **REQ-CMM.PATH.OPT-002** The proposal shall report total transit length and tip-change count before and after.
- **REQ-CMM.PATH.OPT-003** Optimisation shall be deterministic for a given seed (class D2 or better), and the seed shall be recorded in the proposal.
- **REQ-CMM.PATH.OPT-004** The system shall apply a proposal only through a separate acceptance command that checks the plan digest it was computed against is still current.
- **REQ-CMM.PATH.OPT-005** Optimisation of a 500-feature program shall finish within a configurable time budget (default 10 s [unverified]) and return the best order found so far.

## Interfaces

- **Offers:** `cmm.optimize_path@1` (R1 proposal), `cmm.accept_path_optimization@1` (W2); PathOptimizationProposal type.
- **Consumes:** `CMM.PATH.PLAN`; `CMM.COLL` verdicts for re-checking changed transits.
- **Journal:** `cmm.optimize_path@1`, `cmm.accept_path_optimization@1`.

## Decomposition hints

COST (cost model and constraints), SEARCH (seeded heuristic), ACCEPT (proposal and acceptance).

## Open questions

- A time budget makes the result depend on machine speed unless the search is bounded by iteration count; the leaf should bound iterations and treat the time budget as a cap only, recording which bound stopped it.
- Patent freedom-to-operate review before P2 (maintainer action).

## Children

_None yet: status `open`._
