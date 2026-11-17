---
id: CMM.COLL
title: Machine, fixture and collision models
level: 2
parent: CMM
kind: subsystem
status: split
phase: P2
size: XL
owners: []
claimed_by:
satisfies: [REQ-CMM-003]
covers: [PCD.PTH.03, PCD.PTH.06]
depends_on: [GEO, HAL, CMM.PROBE]
cli: [fid cmm *-machine*, fid cmm *-fixture*, fid cmm check-collision*]
mcp: [cmm_*_machine*, cmm_*_fixture*, cmm_check_collision*]
journal: [cmm.*_machine*@*, cmm.*_fixture*@*, cmm.check_collision*@*]
updated: 2026-11-16
---

# CMM.COLL: Machine, fixture and collision models

## Purpose

Owns the **world the probe moves in** and the deterministic answer to "does this move hit anything?". That means the machine model (envelope, kinematics, column and bridge geometry, rotary tables), fixture and part placement models, and the swept-volume collision check of probe, stylus and head against them. It is separate because the same verdict must be produced identically during planning, in simulation and in the Motion Executive's validators, so it must be one deterministic CPU component with one owner.

## Scope

**In:**
- Machine model: travel envelope, axis kinematics (Cartesian 3-axis), column and bridge obstruction geometry, envelope margin; seeded from controller-reported limits plus user-entered geometry
- Rotary tables: axis pose, range and indexing as part of the machine model
- Fixture models (from CAD or primitives) and nominal part placement, as journaled, digested entities
- Swept volumes of probe, stylus, extensions and head for each move, from the probe configuration
- Collision and clearance queries: move segment against machine, fixture, part and rack, with a clearance margin; result as typed verdicts with first contact location
- Distinguishing intended probing contact (within the probing zone of a probe move) from unintended contact
- Model digests used by run tickets (`machine_model`, `fixture`, `part_placement`)

**Out** (and which node owns it instead):
- Building the plan and choosing avoidance moves (owned by `CMM.PATH`)
- Animating and reporting collisions over a full program (owned by `CMM.SIM`)
- Rendering of models and verdicts (owned by `VIZ`)
- Geometry primitives, BVH and distance queries (owned by `GEO`)
- Probe configuration data (owned by `CMM.PROBE`)
- Controller-reported limits over I++ (owned by `HAL`)

## Requirements

- **REQ-CMM.COLL-001** The system shall represent machine, fixture and nominal part-placement models as versioned entities with content digests that change when any geometry, pose or limit changes.
- **REQ-CMM.COLL-002** When controller-reported travel limits are available, the system shall seed the machine envelope from them, and shall reject a user-entered envelope that extends beyond them.
- **REQ-CMM.COLL-003** The system shall model rotary tables with axis pose, angular range and index step, and shall reject any rotary target outside the range.
- **REQ-CMM.COLL-004** For any move segment and probe configuration, the system shall compute whether the swept probe, stylus, extension and head volume intersects or comes within the clearance margin of the machine, fixture, part or rack models.
- **REQ-CMM.COLL-005** The system shall classify contact within a probe move's probing zone as intended and any other contact or margin violation as a collision, reporting the move index, contact point and the bodies involved.
- **REQ-CMM.COLL-006** Collision checks shall run on the CPU in f64 and shall be deterministic (class D0 or D1), so the same plan and models give the same verdicts in planning, simulation and execution validation.
- **REQ-CMM.COLL-007** If a model needed for a check is missing (no fixture model, no machine model), then the check shall return the verdict `UNCHECKED` for the affected moves rather than `CLEAR`.
- **REQ-CMM.COLL-008** A collision check of a 1,000-move plan against models totalling 1 M triangles shall complete in under 5 s on reference hardware [unverified].

## Interfaces

- **Offers:** commands such as `cmm.define_machine@1`, `cmm.define_fixture@1`, `cmm.set_part_placement@1` (W2), `cmm.check_collision@1` (R1); data types MachineModel, RotaryAxis, FixtureModel, SweptVolume, CollisionVerdict (`CLEAR | COLLISION | MARGIN | UNCHECKED`).
- **Consumes:** `GEO` meshes, CAD, BVH and distance queries; `CMM.PROBE` probe geometry; `HAL` controller-reported limits.
- **Journal:** `cmm.*_machine*`, `cmm.*_fixture*`, `cmm.check_collision*`.

## Decomposition hints

MACH (machine model and envelope), ROT (rotary tables), FIXT (fixtures and part placement), SWEEP (swept probe volumes), CHECK (collision queries and verdicts), PERF (performance budget and corpus).

## Open questions

- The stated limitation stands: collision checking is only as good as the setup model, and true part placement is known only after alignment. This is why `CMM.EXEC` requires setup runs and per-cycle placement checks.
- Articulating-head and rotary kinematics beyond 3-axis Cartesian are marked P3 in the concept docs; rotary tables are in this domain's scope, so a phase decision may be needed for their children.

## Split rationale

The split separates the **world models** from the **query** that uses them. MACH, ROT and FIXT each own one kind of world entity with its own digest (the ticket's `machine_model`, `fixture` and `part_placement`); MACH and FIXT are leaves. SWEEP owns the conservative probe-assembly geometry, and CHECK owns the single deterministic verdict that planning, simulation and the Motion Executive share. CHECK takes generic move segments rather than the plan type, so `CMM.PATH` depends on `CMM.COLL` and not the reverse. Performance (decomposition hint PERF) stays inside CHECK because the budget is a property of the query.

Alternatives considered:
- **SWEEP inside CHECK.** Rejected: the over-approximation bound is a correctness argument of its own and deserves separate review.
- **Rotary tables inside MACH.** Rejected: rotary kinematics are proposed for P3 (D-018), while the 3-axis model is needed in P2.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [CMM.COLL.MACH](mach/NODE.md) | Machine model, envelope and obstruction bodies | 001 002 | PCD.PTH.06 |
| [CMM.COLL.ROT](rot/NODE.md) | Rotary tables in the machine model | 001 003 | |
| [CMM.COLL.FIXT](fixt/NODE.md) | Fixture models and nominal part placement | 001 | PCD.PTH.06 |
| [CMM.COLL.SWEEP](sweep/NODE.md) | Swept volumes of the probe assembly | 004 | |
| [CMM.COLL.CHECK](check/NODE.md) | Collision and clearance queries and verdicts | 004 005 006 007 008 | PCD.PTH.03 |
