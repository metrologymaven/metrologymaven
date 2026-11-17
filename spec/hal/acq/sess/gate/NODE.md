---
id: HAL.ACQ.SESS.GATE
title: Driver-call gate (no state change without a journaled command)
level: 4
parent: HAL.ACQ.SESS
kind: task
status: leaf
phase: P2
size: S
owners: []
claimed_by:
satisfies: [REQ-HAL.ACQ.SESS-003]
covers: []
depends_on: [HAL.IDL.BASE, CORE.BUS.PIPE]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# HAL.ACQ.SESS.GATE: Driver-call gate

## Purpose

The single choke point through which the runtime calls a driver. A state-changing call passes only when it carries a command context issued by the bus for a journaled D1 command, or an M1 command issued by the Motion Executive. Read-only calls (describe, health, show) pass without one. This is how "no side doors" is enforced for devices.

## Scope

**In:**
- `DriverGate::call(ctx: Option<&CommandContext>, op: DriverOp)` and the classification of every driver operation as read-only, D1 or M1 (taken from the capability package's per-operation class)
- Rejection with `no_command_context` and `class_mismatch`
- An M1 check: the context's origin must be the Motion Executive's motion path

**Out:**
- Issuing command contexts (owned by `CORE.BUS`)
- Motion approval (owned by `CMM` execution)
- Driver transport (owned by `HAL.HOST`)

## Requirements

- **REQ-HAL.ACQ.SESS.GATE-001** If a state-changing driver operation is called without a command context, then the gate shall refuse it with `no_command_context` and make no driver call.
- **REQ-HAL.ACQ.SESS.GATE-002** If the command context's class is lower than the operation's class (for example an R0 command context for a D1 operation, or a D1 context for an M1 operation), then the gate shall refuse with `class_mismatch`.
- **REQ-HAL.ACQ.SESS.GATE-003** If an M1 operation carries a context whose origin is not the Motion Executive's motion path, then the gate shall refuse with `class_mismatch`.
- **REQ-HAL.ACQ.SESS.GATE-004** Read-only driver operations shall pass the gate without a command context.
- **REQ-HAL.ACQ.SESS.GATE-005** Every refusal shall be recorded as a `device.event` with the operation name and the refusing rule.

## Acceptance criteria

- **AC-1** Given a mock driver and `set_configuration` with no context, when called through the gate, then it fails with `no_command_context` and the mock records zero calls.
- **AC-2** Given an R0 context and `start` (D1), then `class_mismatch`.
- **AC-3** Given a D1 context from the CLI and a `CncMotion.goto` (M1), then `class_mismatch`; given an M1 context from the Motion Executive test double, the call passes.
- **AC-4** Given `get_health` with no context, then the call passes.
- **AC-5** Given each refusal above, then a `device.event` names the operation and rule.

## Design

- **Registry commands:** none; wraps every driver call made by `HAL.ACQ.SESS.LIFE`, `HAL.ACQ.SESS.CFG`, `HAL.ACQ.INGEST.CMDS` and `HAL.IPP`.
- **Data structures:** `CommandContext { command_id, class, principal, origin }` (issued by `CORE.BUS`), `DriverOp { capability, op, class }`.
- **Algorithm:** class lattice R0 < R1 < I1 < D1 < M1 for this check; table lookup of the operation class from the negotiated package.
- **Errors:** `no_command_context`, `class_mismatch`.
- **Determinism:** D0.
- **Performance budget:** under 1 us per call [unverified until measured].
- **Crate:** `fid-hal-runtime` (module `gate`), layer L2.

## Test fixtures

Mock driver and a Motion Executive test double in the crate's test support.

## Build prompt

> In crate `fid-hal-runtime`, module `gate`, implement `DriverGate::call(ctx: Option<&CommandContext>, op: DriverOp) -> Result<Reply, GateError>`. Read-only ops pass without context. State-changing ops require a context (`no_command_context`) whose class is at least the op's class in the lattice R0 < R1 < I1 < D1 < M1 (`class_mismatch`); M1 ops also require origin = Motion Executive motion path. Op classes come from the negotiated capability package. Record each refusal as a `device.event`. No panic, unwrap, expect or unsafe (CONSTITUTION §8). Tests: AC-1..AC-5 with a mock driver.

## Open questions

- The class lattice used here is a gate ordering, not a constitutional ordering of side-effect classes; `CORE.BUS` may supply its own comparator instead.

## Children

_Leaf: none._
