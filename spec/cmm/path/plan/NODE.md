---
id: CMM.PATH.PLAN
title: Motion plan model, compilation and digest
level: 3
parent: CMM.PATH
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-CMM.PATH-001, REQ-CMM.PATH-002]
covers: []
depends_on: [GEO, CMM.STRAT, CMM.PROBE, CMM.PATH.ANGLE, CMM.PATH.CLEAR, CMM.PATH.CAP]
cli: [fid cmm plan-path, fid cmm show-path]
mcp: [cmm_plan_path, cmm_show_path]
journal: [cmm.plan_path@1]
updated: 2026-11-16
---

# CMM.PATH.PLAN: Motion plan model, compilation and digest

## Purpose

Defines the motion plan, the artifact that the collision checker, simulator and Motion Executive all consume byte for byte, and compiles a measurement program plus its strategies into it. A plan is an ordered list of typed moves (transit, approach, probe, retract, scan, tip change, head index, rotary index) with target poses in SI metres or radians and velocity fields, plus the digests of every input. Compilation is deterministic and the plan digest is computed over the canonical form.

## Scope

**In:**
- `MotionPlan`, `Move` and `PlanDigest` types and their canonical JSON form
- Compilation pipeline: per-feature approach, probe and retract moves from hit and scan targets; search distances; tip-change and head-index moves; rotary-index moves from the machine model; transits via `CMM.PATH.CLEAR`; caps via `CMM.PATH.CAP`
- Compilation to a machine-independent, I++-level move list
- Input digests recorded in the plan (program, strategies, probe configuration, machine model)
- `cmm.plan_path@1` (R1) and `cmm.show_path@1` (R0)

**Out** (and which node owns it instead):
- Clearance surfaces and move points (owned by `CMM.PATH.CLEAR`)
- Head-angle and tip selection (owned by `CMM.PATH.ANGLE`)
- Velocity caps and clamps (owned by `CMM.PATH.CAP`)
- Ordering optimisation (owned by `CMM.PATH.OPT`)
- Collision verdicts (owned by `CMM.COLL`)

## Requirements

- **REQ-CMM.PATH.PLAN-001** The system shall compile a measurement program and its strategies into a motion plan of typed moves (transit, approach, probe, retract, scan, tip change, head index, rotary index), each with a target pose in SI metres or radians and a velocity class.
- **REQ-CMM.PATH.PLAN-002** For each hit target, the system shall emit an approach move to the configured approach distance along the approach vector, a probe move to the configured search distance beyond the nominal, and a retract move back to the approach point.
- **REQ-CMM.PATH.PLAN-003** The system shall record in the plan the digests of the program, every strategy, the probe configuration, the head model and the machine model used.
- **REQ-CMM.PATH.PLAN-004** Plan compilation shall be deterministic (class D0), and the plan digest shall be SHA-256 over the RFC 8785 canonical JSON of the plan.
- **REQ-CMM.PATH.PLAN-005** The system shall express every move at the I++ level (target position, tool direction, velocity) without controller-specific commands, so the same plan runs on any I++ server.

## Interfaces

- **Offers:** `cmm.plan_path@1` (R1), `cmm.show_path@1` (R0); MotionPlan, Move, PlanDigest types consumed by `CMM.COLL`, `CMM.SIM` and `CMM.EXEC`.
- **Consumes:** `CMM.STRAT` targets; `CMM.PROBE` configuration and head; `CMM.PATH.ANGLE`, `CMM.PATH.CLEAR`, `CMM.PATH.CAP`; `GEO` frames.
- **Journal:** `cmm.plan_path@1`.

## Decomposition hints

TYPES (plan and move model and canonical form), HITS (approach, probe, retract generation), SCANS (scan move generation), TOOL (tip change and head index moves), COMPILE (pipeline and digest).

## Open questions

- Whether search distance and approach distance defaults belong to the strategy or to the plan is a domain choice; they are plan parameters here.

## Children

_None yet: status `open`._
