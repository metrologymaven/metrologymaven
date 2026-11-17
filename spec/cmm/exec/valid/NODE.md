---
id: CMM.EXEC.VALID
title: Motion Executive validator suite and simulation gate
level: 3
parent: CMM.EXEC
kind: module
status: open
phase: P2
size: XL
owners: []
claimed_by:
satisfies: [REQ-CMM.EXEC-007, REQ-CMM.EXEC-008]
covers: []
depends_on: [CMM.COLL.CHECK, CMM.PROBE.VALID, CMM.PROBE.HEAD, CMM.SIM.GATE, AUTO.PROG]
cli: [fid cmm validate-run]
mcp: [cmm_validate_run]
journal: [cmm.validate_run@1]
updated: 2026-11-16
---

# CMM.EXEC.VALID: Motion Executive validator suite and simulation gate

## Purpose

The deterministic checks the Motion Executive runs before it issues a ticket and again before it arms: travel limits with margin, velocity and acceleration against the ticket cap and tenant maximum, collision (through the shared collision checker), probe and tool state, articulation, program structure, and the simulation gate. Each validator returns a typed pass or fail with evidence; any failure refuses the ticket or arming. The suite is versioned and its version is bound in the ticket.

## Scope

**In:**
- Validator suite model, version and `ValidatorReport`
- Validators: travel limits (envelope minus margin, cross-checked with controller-reported limits), velocity and acceleration (cap, tenant maximum, stylus rules), collision, probe and tool state (qualified, unexpired, within drift, rack positions known), articulation (angles reachable, indexable, qualified), program structure (bounded loops, all motion plan-derived)
- Simulation gate: a clean report for the byte-identical plan with the same machine, setup and probe digests, and the accepted simulator fidelity levels
- `cmm.validate_run@1` (R1) for dry checks by programmers and agents

**Out** (and which node owns it instead):
- Collision computation (owned by `CMM.COLL.CHECK`)
- Clean-simulation lookup (owned by `CMM.SIM.GATE`)
- Probe status (owned by `CMM.PROBE.VALID`)
- Per-cycle placement check (owned by `CMM.EXEC.SCOPE`)

## Requirements

- **REQ-CMM.EXEC.VALID-001** The Motion Executive shall refuse to issue a ticket unless the simulation gate returns a clean report for the byte-identical plan digest with the same machine, setup and probe configuration digests, at an accepted fidelity level.
- **REQ-CMM.EXEC.VALID-002** The Motion Executive shall run every validator in the suite (travel limits with margin, velocity and acceleration, collision, probe and tool state, articulation, program structure) and shall refuse to issue or arm if any validator fails.
- **REQ-CMM.EXEC.VALID-003** Each validator shall be deterministic and shall return a typed result with the evidence (first failing move, value, limit) in a ValidatorReport bound to the suite version.
- **REQ-CMM.EXEC.VALID-004** The same validator code shall be callable as `cmm.validate_run@1` (R1) outside the Motion Executive, and its result shall be advisory there; only the Motion Executive's own run counts for a ticket.
- **REQ-CMM.EXEC.VALID-005** If any validator cannot run (missing model, missing status), then it shall return `UNVALIDATED`, which counts as a failure.

## Interfaces

- **Offers:** ValidatorSuite, ValidatorReport; `cmm.validate_run@1` (R1).
- **Consumes:** `CMM.COLL.CHECK`, `CMM.PROBE.VALID`, `CMM.PROBE.HEAD`, `CMM.SIM.GATE`, `AUTO.PROG` static checker; tenant limits from `CORE`.
- **Journal:** `cmm.validate_run@1`.

## Decomposition hints

SUITE (suite model and report), LIMITS (travel and velocity), STATE (probe, tool and articulation), STRUCT (program structure), SIMGATE (simulation gate and fidelity policy).

## Open questions

- Accepted fidelity levels for the gate (L1 only, or L1 or higher) are a policy choice carried from `CMM.SIM`; L1 only is the conservative default until decided.
- Stylus-specific approach, retract and probing speed rules need a domain expert.

## Children

_None yet: status `open`._
