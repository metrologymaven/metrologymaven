---
id: CMM.PATH.CAP
title: Velocity cap compilation and clamp records
level: 3
parent: CMM.PATH
kind: task
status: leaf
phase: P2
size: S
owners: []
claimed_by:
satisfies: [REQ-CMM.PATH-009]
covers: []
depends_on: []
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# CMM.PATH.CAP: Velocity cap compilation and clamp records

## Purpose

Compiles an absolute commanded-velocity cap into a motion plan: every move whose commanded velocity exceeds the cap is clamped to it, every clamp is recorded in the plan, and the cap itself becomes part of the plan, so the plan digest binds it. The Motion Executive supplies the cap from the ticket's speed scope (setup cap or approved cap); this task only applies it.

## Scope

**In:**
- `apply_velocity_cap(moves, cap_m_s) -> (moves, Vec<ClampRecord>)`
- The `velocity_cap_m_s` field of the plan header and its inclusion in the digest

**Out** (and which node owns it instead):
- Choosing the cap (setup or approved) (owned by `CMM.EXEC.SCOPE`)
- Validating velocities at arming (owned by `CMM.EXEC.VALID`)
- The plan type (owned by `CMM.PATH.PLAN`; this task adds the header field and the clamp list)

## Requirements

- **REQ-CMM.PATH.CAP-001** When a cap is supplied, the system shall set every move's commanded velocity to the minimum of its velocity and the cap.
- **REQ-CMM.PATH.CAP-002** The system shall record one clamp record per clamped move with move index, original velocity and clamped velocity.
- **REQ-CMM.PATH.CAP-003** The system shall store the cap in the plan header so the plan digest changes when the cap changes.
- **REQ-CMM.PATH.CAP-004** If the cap is non-finite or non-positive, then the system shall fail with `VELOCITY_CAP_INVALID` and leave the plan unchanged.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/cmm/path/plan_10_moves.json` (velocities 0.01 to 0.10 m/s) and a cap of 0.05 m/s, when `apply_velocity_cap` runs, then moves 6 to 10 are clamped to 0.05 m/s, five clamp records exist with the original velocities, and moves 1 to 5 are unchanged.
- **AC-2** Given the AC-1 result and the same plan capped at 0.03 m/s, when both plans are digested, then the digests differ.
- **AC-3** Given a cap of 1.0 m/s, when applied, then no move changes, no clamp record exists, and the header holds the cap.
- **AC-4** (error path) Given a cap of 0 or NaN, when applied, then it fails with `VELOCITY_CAP_INVALID`.
- **AC-5** (replay) Given AC-1 run twice, then the outputs are bit-identical.

## Design

- **Interface:** `fn apply_velocity_cap(plan: &MotionPlan, cap_m_s: f64) -> Result<(MotionPlan, Vec<ClampRecord>), CapError>`; side-effect class R1 (pure).
- **Data structures:** `ClampRecord { move_index: u32, original_m_s: f64, clamped_m_s: f64 }`; plan header field `velocity_cap_m_s: f64`.
- **Algorithm:** single pass in move order with total-order min.
- **Errors:** `VELOCITY_CAP_INVALID`.
- **Determinism:** D0.
- **Crate:** `fid-cmm-path`.

## Test fixtures

`fixtures/cmm/path/plan_10_moves.json`, a hand-written ten-move plan, CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-cmm-path`, implement `apply_velocity_cap(plan, cap_m_s)` (pure, side-effect class R1). Clamp each move's commanded velocity to min(velocity, cap) in move order, record a `ClampRecord {move_index, original_m_s, clamped_m_s}` for each clamp, and store `velocity_cap_m_s` in the plan header so the RFC 8785 JCS + SHA-256 plan digest changes with the cap. Non-finite or non-positive caps fail with `VELOCITY_CAP_INVALID`.
> Acceptance tests: the five criteria in spec node `CMM.PATH.CAP`, fixture `fixtures/cmm/path/plan_10_moves.json`.
> Constraints: total-order min; NaN is an error; no panic, no `unsafe`, no `unwrap`/`expect`; determinism D0. This cap is process assurance, not a safety function.

## Open questions

- None.

## Children

_Leaf: none._
