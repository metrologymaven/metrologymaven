---
id: HAL.ACQ.SESS.HEALTH
title: Health supervision and error-to-behaviour mapping
level: 4
parent: HAL.ACQ.SESS
kind: feature
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-HAL.ACQ.SESS-005]
covers: []
depends_on: [HAL.ACQ.SESS.LIFE, HAL.IDL.BASE.HEALTH, HAL.IDL.BASE.ERR]
side_effect: R0
determinism: D0
cli: [fid device show-health]
mcp: [device_show_health]
journal: [device.show_health@1]
updated: 2026-11-17
---

# HAL.ACQ.SESS.HEALTH: Health supervision and error-to-behaviour mapping

## Purpose

Tracks each session's device health against the `Device@1` state machine and turns every device error into one defined runtime behaviour: retry, interrupt, stop the stream or fail. It is the only component that calls `interrupt` on a session.

## Scope

**In:**
- Health tracking per session from driver health reports and errors, validated against the transition table of `HAL.IDL.BASE.HEALTH`
- The runtime behaviour table: one behaviour per error code of `HAL.IDL.BASE.ERR`
- Bounded retry for `TRANSIENT` (3 attempts, exponential back-off from 50 ms [unverified])
- `device.event` records for every health transition and every applied behaviour
- `device.show_health@1` (R0)

**Out:**
- The state machine and error codes themselves (owned by `HAL.IDL.BASE.HEALTH` and `HAL.IDL.BASE.ERR`)
- Session state changes other than interrupt (owned by `HAL.ACQ.SESS.LIFE`)
- Host process restarts (owned by `HAL.HOST.SUPER`)
- Clearing safety stops (never done in software; reset happens at the machine)

## Requirements

- **REQ-HAL.ACQ.SESS.HEALTH-001** When a driver reports `COMM_LOST`, the supervisor shall call `interrupt` on the session, so that it becomes `INTERRUPTED` and requires an explicit resume or close.
- **REQ-HAL.ACQ.SESS.HEALTH-002** When a driver reports `SAFETY_STOP_OBSERVED`, the supervisor shall stop every active stream on the session, mark the health `SAFETY_STOPPED`, and offer no software path that leaves `SAFETY_STOPPED`.
- **REQ-HAL.ACQ.SESS.HEALTH-003** When a driver reports `TRANSIENT` on a retryable operation, the supervisor shall retry at most 3 times with exponential back-off, and then apply the `FAULT` behaviour.
- **REQ-HAL.ACQ.SESS.HEALTH-004** If a driver reports a health transition outside the transition table, then the supervisor shall record a `device.event` with reason `illegal_transition`, set health to `FAULT`, and interrupt the session.
- **REQ-HAL.ACQ.SESS.HEALTH-005** Every error code of the `Device@1` error model shall map to exactly one behaviour in a table that is checked for completeness at build time.
- **REQ-HAL.ACQ.SESS.HEALTH-006** When `device.show_health@1` runs, the system shall return the current health state, the last 32 transitions and the last error per session.

## Acceptance criteria

- **AC-1** Given a mock driver reporting `COMM_LOST`, then the session is `INTERRUPTED` and a `device.event` records the transition.
- **AC-2** Given `SAFETY_STOP_OBSERVED` during a stream, then the stream is stopped with end reason `safety_stop`, health is `SAFETY_STOPPED`, and no registry command in the manifest changes it.
- **AC-3** Given `TRANSIENT` four times in a row, then three retries occur with back-off 50, 100, 200 ms (mock clock) and the behaviour becomes `FAULT`.
- **AC-4** (error path) Given a reported transition `OFFLINE -> BUSY` (not in the table), then `illegal_transition` is recorded, health is `FAULT` and the session is interrupted.
- **AC-5** Given the error-code enum extended by one variant without a behaviour row, then the build fails (exhaustive match test).
- **AC-6** Given `fid device show-health --json`, then the output holds state, the last transitions (at most 32) and the last error.

## Design

- **Registry commands:** `device.show_health@1` (R0; params `session_id`; result `HealthView`).
- **Data structures:** `Supervisor { per_session: BTreeMap<SessionId, HealthTrack> }`, `HealthTrack { state, ring: [Transition; 32], last_error }`, `Behaviour { Retry, Interrupt, StopStreams, Fault, Fail }`.
- **Behaviour table:** taken from `ErrorCode::behaviour()` in `HAL.IDL.BASE.ERR` (TRANSIENT Retry; COMM_LOST Interrupt; CONFIG_INVALID, OUT_OF_RANGE, NOT_QUALIFIED, CALIBRATION_EXPIRED, VENDOR_LICENSE, UNSUPPORTED Fail; LIMIT StopStreams; SAFETY_STOP_OBSERVED StopStreams plus `SAFETY_STOPPED`; INTERNAL Fault). This leaf implements the behaviours; it does not redefine them. Calibration codes at start are decided by `HAL.ACQ.POLICY` (flag or block).
- **Algorithm:** transition validation by table lookup; back-off on an injected clock.
- **Errors:** none raised to callers beyond the behaviours.
- **Determinism:** D0 given the same report sequence and clock.
- **Performance budget:** trivial.
- **Crate:** `fid-hal-runtime` (module `session::health`), layer L2.

## Test fixtures

Mock driver scripts under `fixtures/hal/acq/sess/health/` (CC-BY-4.0); mock clock.

## Build prompt

> In crate `fid-hal-runtime`, module `session::health`, implement a `Supervisor` that consumes driver health reports and errors per session, validates transitions against the `HAL.IDL.BASE.HEALTH` table (illegal -> `device.event` `illegal_transition`, health `FAULT`, interrupt), and applies `ErrorCode::behaviour()` from `fid-hal-idl` exhaustively (TRANSIENT retry x3 with 50/100/200 ms back-off on an injected clock then FAULT; COMM_LOST interrupt; SAFETY_STOP_OBSERVED stop streams and set `SAFETY_STOPPED` with no software exit; LIMIT stop streams; others as in Design). Record every transition and behaviour as a `device.event`. Add `device.show_health@1` (R0; CLI `fid device show-health`, MCP `device_show_health`). No panic, unwrap, expect or unsafe; BTreeMap only (CONSTITUTION §8). Tests: AC-1..AC-6.

## Open questions

- The retry counts and back-off are proposals [unverified].

## Children

_Leaf: none._
