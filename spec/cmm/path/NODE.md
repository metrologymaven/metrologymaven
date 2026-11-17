---
id: CMM.PATH
title: Path planning, clearance and optimisation
level: 2
parent: CMM
kind: subsystem
status: split
phase: P2
size: XL
owners: []
claimed_by:
satisfies: [REQ-CMM-003]
covers: [PCD.PTH.01, PCD.PTH.02, PCD.PTH.05]
depends_on: [GEO, CMM.STRAT, CMM.PROBE, CMM.COLL]
cli: [fid cmm *-path*, fid cmm *-clearance*, fid cmm *-move*]
mcp: [cmm_*_path*, cmm_*_clearance*, cmm_*_move*]
journal: [cmm.*_path*@*, cmm.*_clearance*@*, cmm.*_move*@*]
updated: 2026-11-16
---

# CMM.PATH: Path planning, clearance and optimisation

## Purpose

Turns strategies into a **motion plan**: approach, probe and retract moves for every hit or scan, move points, clearance planes and cylinders, head-angle and tip choices, tip changes and rotary-table indexing, and the transits between features. It also orders features to shorten the plan and generates a path automatically from a selected feature list. The output is a deterministic, digestable motion plan that the collision checker, simulator and Motion Executive all consume byte-for-byte.

## Scope

**In:**
- Move points (user-inserted waypoints) and clearance planes and cylinders, per program and per feature
- Approach, retract and search distances and speed classes per hit
- Head-angle and tip selection per feature (from qualified angles only)
- Tip-change and rack moves in the plan
- Rotary-table indexing steps in the plan (positions from the machine model)
- Automatic path from a selected feature list
- Path optimisation: feature ordering and transit minimisation subject to clearance and collision constraints
- Compilation of the plan to a machine-independent, I++-level move list with a stable digest
- Velocity fields in the plan, including the absolute velocity cap compiled from the ticket's speed scope

**Out** (and which node owns it instead):
- Hit layouts and scan parameters (owned by `CMM.STRAT`)
- Collision verdicts, swept volumes and machine and fixture models (owned by `CMM.COLL`)
- Animation and simulation (owned by `CMM.SIM`)
- Sending moves to the controller, look-ahead and holds (owned by `CMM.EXEC`)
- Program assembly from CAD, PMI or DMIS (owned by `CMM.GEN`)

## Requirements

- **REQ-CMM.PATH-001** The system shall compile a measurement program and its strategies into a motion plan of typed moves (transit, approach, probe, retract, scan, tip change, head index, rotary index), each with target pose in SI metres or radians and a velocity class.
- **REQ-CMM.PATH-002** Plan compilation shall be deterministic (class D0), and the plan's digest shall be computed over its canonical form (RFC 8785 JCS + SHA-256).
- **REQ-CMM.PATH-003** The system shall insert transits so that every move between features passes through the active clearance plane or cylinder unless a user move point overrides it.
- **REQ-CMM.PATH-004** When the user inserts, edits or deletes a move point, the system shall recompile the affected plan segment and journal the change as one command.
- **REQ-CMM.PATH-005** The system shall choose head angles only from angles the probe model marks qualified and valid, and shall report a feature as unreachable when no such angle gives an approach within the configured angular limit of the hit normal.
- **REQ-CMM.PATH-006** When given a list of selected features, the system shall generate a complete collision-checked path for them, and shall return as a draft any feature it could not path, with the reason.
- **REQ-CMM.PATH-007** When path optimisation is requested, the system shall reorder features to reduce total transit length without violating clearance or collision constraints and without reordering steps the program marks as order-dependent, and shall report the before and after length.
- **REQ-CMM.PATH-008** Path optimisation shall be deterministic for a given seed (class D2 or better).
- **REQ-CMM.PATH-009** If any move's commanded velocity would exceed the velocity cap supplied for compilation, then the system shall clamp it to the cap and record the clamp in the plan.

## Interfaces

- **Offers:** commands such as `cmm.plan_path@1` (R1), `cmm.set_clearance@1`, `cmm.insert_move_point@1` (W2), `cmm.optimize_path@1` (R1 proposal, W2 to accept), `cmm.auto_path@1` (W2 draft); data types MotionPlan, Move, ClearanceSurface, PlanDigest.
- **Consumes:** `CMM.STRAT` hit and scan targets; `CMM.PROBE` qualified angles and tips; `CMM.COLL` collision queries during planning; `GEO` frames and transforms.
- **Journal:** `cmm.*_path*`, `cmm.*_clearance*`, `cmm.*_move*`.

## Decomposition hints

PLAN (plan model and compilation), CLEAR (clearance surfaces and move points), ANGLE (head-angle and tip selection), AUTO (auto path from features), OPT (ordering optimisation), CAP (velocity fields and caps).

## Open questions

- The concept docs require a path-planning patent freedom-to-operate review before P2 work starts; that is a maintainer action, not a spec choice.
- Auto-path from PMI and the path editor are P3 in the Design Baseline §8; children for those may need a phase decision.

## Split rationale

The split separates the **plan artifact** from the **decisions that shape it**. PLAN owns the model, the compilation pipeline and the digest that the collision checker, simulator and Motion Executive bind to. CLEAR (safe travel geometry and move points), ANGLE (which tip and angle) and CAP (velocity clamping) are inputs to compilation with their own small, testable rules; ANGLE and CAP are leaves. AUTO and OPT are the two higher-level planners that iterate over PLAN and the collision checker: AUTO makes a path exist, OPT makes it shorter. Keeping CAP apart from PLAN keeps the speed policy's compiled form auditable on its own.

Alternatives considered:
- **CAP inside PLAN.** Rejected: the cap is what the Motion Executive's setup scope relies on, so its clamp rule should be reviewable in isolation.
- **AUTO and OPT merged.** Rejected: OPT is a proposal-then-accept flow with seeded search; AUTO is a deterministic remedy loop producing a draft.

Phase: all children P2. A graphical path editor (P3 in the Design Baseline) is out of these children; if one is added it is proposed for P3 in D-018.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [CMM.PATH.PLAN](plan/NODE.md) | Motion plan model, compilation and digest | 001 002 | |
| [CMM.PATH.CLEAR](clear/NODE.md) | Clearance surfaces, move points and transits | 003 004 | PCD.PTH.01 |
| [CMM.PATH.ANGLE](angle/NODE.md) | Head-angle and tip selection per feature | 005 | |
| [CMM.PATH.AUTO](auto/NODE.md) | Automatic path from a selected feature list | 006 | PCD.PTH.05 |
| [CMM.PATH.OPT](opt/NODE.md) | Feature ordering optimisation | 007 008 | PCD.PTH.02 |
| [CMM.PATH.CAP](cap/NODE.md) | Velocity cap compilation and clamp records | 009 | |
