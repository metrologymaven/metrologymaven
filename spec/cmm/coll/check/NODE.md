---
id: CMM.COLL.CHECK
title: Collision and clearance queries and verdicts
level: 3
parent: CMM.COLL
kind: module
status: open
phase: P2
size: XL
owners: []
claimed_by:
satisfies: [REQ-CMM.COLL-004, REQ-CMM.COLL-005, REQ-CMM.COLL-006, REQ-CMM.COLL-007, REQ-CMM.COLL-008]
covers: [PCD.PTH.03]
depends_on: [GEO, CMM.COLL.MACH, CMM.COLL.FIXT, CMM.COLL.SWEEP, CMM.PROBE.RACK]
cli: [fid cmm check-collision]
mcp: [cmm_check_collision]
journal: [cmm.check_collision@1]
updated: 2026-11-16
---

# CMM.COLL.CHECK: Collision and clearance queries and verdicts

## Purpose

Answers "does this move hit anything?" for a list of move segments: it tests each segment's swept probe volume against the machine, fixture, part and rack models with a clearance margin, distinguishes intended probing contact from collisions, and returns a typed verdict per move (`CLEAR`, `COLLISION`, `MARGIN`, `UNCHECKED`) with the first contact. The same component, with the same inputs, must give the same verdicts in planning, simulation and Motion Executive validation.

## Scope

**In:**
- `cmm.check_collision@1` (R1) over a list of typed move segments (independent of the plan type, so `CMM.PATH` can depend on this node and not the reverse)
- World assembly from machine, rotary (when present), fixture, part placement and rack models
- Broad phase (BVH from `GEO`) and narrow phase (primitive-primitive and primitive-mesh distance)
- Probing-zone rule: contact of the active tip with the part inside a probe move's probing zone is intended
- `UNCHECKED` when a needed model is missing
- Performance corpus and budget

**Out** (and which node owns it instead):
- Swept volumes (owned by `CMM.COLL.SWEEP`)
- Model entities (owned by `CMM.COLL.MACH`, `CMM.COLL.ROT`, `CMM.COLL.FIXT`, `CMM.PROBE.RACK`)
- Avoidance and replanning (owned by `CMM.PATH.AUTO`)
- Whole-program reports and animation (owned by `CMM.SIM`)

## Requirements

- **REQ-CMM.COLL.CHECK-001** For each move segment, the system shall test the swept assembly against machine, fixture, part and rack bodies and shall return `COLLISION` on intersection, `MARGIN` when the minimum distance is below the clearance margin without intersection, and `CLEAR` otherwise.
- **REQ-CMM.COLL.CHECK-002** The system shall classify contact between the active tip and the part within a probe move's probing zone (from the approach point to the search limit) as intended, and any other contact as a collision.
- **REQ-CMM.COLL.CHECK-003** For each non-`CLEAR` verdict, the system shall report the move index, the first contact point (smallest segment parameter), the assembly element and the body involved.
- **REQ-CMM.COLL.CHECK-004** If the machine, fixture or part-placement model needed for a move is missing, then the system shall return `UNCHECKED` for that move and never `CLEAR`.
- **REQ-CMM.COLL.CHECK-005** Checks shall run on the CPU in f64 with fixed traversal order and shall be deterministic (class D0 or D1), and the result shall carry the digests of all models and the swept-volume bound used.
- **REQ-CMM.COLL.CHECK-006** A check of a 1,000-move plan against models totalling 1 M triangles shall complete in under 5 s on reference hardware [unverified], measured on a published corpus.

## Interfaces

- **Offers:** `cmm.check_collision@1` (R1); CollisionVerdict (`CLEAR | COLLISION | MARGIN | UNCHECKED`), CollisionReport; a library API used by `CMM.PATH`, `CMM.SIM` and `CMM.EXEC.VALID`.
- **Consumes:** `GEO` BVH and distance queries; `CMM.COLL.SWEEP`; model entities from `CMM.COLL.MACH`, `CMM.COLL.FIXT`, `CMM.COLL.ROT`, `CMM.PROBE.RACK`.
- **Journal:** `cmm.check_collision@1`.

## Decomposition hints

WORLD (world assembly and digests), BROAD (broad phase), NARROW (narrow phase and distances), ZONE (probing-zone classification), VERDICT (verdict and report), PERF (corpus and budget).

## Open questions

- The stated limitation stands: collision checking is only as good as the setup model; true part placement is known only after alignment.
- Whether D0 is achievable with `GEO`'s distance queries or D1 is the honest class depends on `GEO`'s determinism; the Motion Executive needs the class stated.

## Children

_None yet: status `open`._
