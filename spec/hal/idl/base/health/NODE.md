---
id: HAL.IDL.BASE.HEALTH
title: Health state machine and transition table
level: 4
parent: HAL.IDL.BASE
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-HAL.IDL.BASE-002, REQ-HAL.IDL.BASE-005]
covers: []
depends_on: [HAL.IDL.BASE.ERR]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
built_in: [fid-hal-idl]
updated: 2026-11-22
---

# HAL.IDL.BASE.HEALTH: Health state machine and transition table

## Purpose

The ten health states every device reports and the closed table of allowed transitions, including who may cause each one (the device or the runtime). It settles the open `MAINTENANCE` question and fixes the safety rule: no software path leaves `SAFETY_STOPPED`.

## Scope

**In:**
- `HealthState`: `OFFLINE`, `CONNECTING`, `INITIALIZING`, `WARMING_UP`, `READY`, `BUSY`, `FAULT`, `SAFETY_STOPPED`, `MAINTENANCE`, `INTERRUPTED`
- `Transition { from, to, by: Device | Runtime }` and the closed table
- `check(from, to, by) -> Ok | IllegalTransition`
- The mapping from health to the stamp's warm-up state (`WARMING_UP | READY`)

**Out:**
- Supervising sessions with the table (owned by `HAL.ACQ.SESS.HEALTH`)
- Error codes (owned by `HAL.IDL.BASE.ERR`)
- Executing the conformance check (owned by `HAL.SIM.CONF`)

## Requirements

- **REQ-HAL.IDL.BASE.HEALTH-001** The transition table shall be closed: `check` shall return `IllegalTransition` for every (from, to, by) triple not listed in Design.
- **REQ-HAL.IDL.BASE.HEALTH-002** No transition out of `SAFETY_STOPPED` shall be allowed with `by: Runtime`; the only exit shall be `SAFETY_STOPPED -> INITIALIZING` reported by the device after a reset at the machine.
- **REQ-HAL.IDL.BASE.HEALTH-003** `MAINTENANCE` shall be enterable by the device from any state except `BUSY` and `SAFETY_STOPPED`, and by the runtime only from `OFFLINE` (for a driver update with no session); its only exit shall be to `OFFLINE`.
- **REQ-HAL.IDL.BASE.HEALTH-004** `INTERRUPTED` shall be entered only with `by: Runtime` (on `COMM_LOST`), and left only to `CONNECTING` (resume) or `OFFLINE` (close).
- **REQ-HAL.IDL.BASE.HEALTH-005** The warm-up state written into stamps shall be `WARMING_UP` while health is `WARMING_UP` and `READY` in `READY` or `BUSY`; acquisition in any other state shall be refused by the package contract.

## Acceptance criteria

- **AC-1** Given every one of the 10 x 10 x 2 triples, when `check` runs, then exactly the triples listed in `fixtures/hal/idl/base/health/table.json` are `Ok` (exhaustive test).
- **AC-2** (error path) Given `(SAFETY_STOPPED, READY, Runtime)` and `(SAFETY_STOPPED, OFFLINE, Runtime)`, then both are `IllegalTransition`.
- **AC-3** Given `(SAFETY_STOPPED, INITIALIZING, Device)`, then `Ok`.
- **AC-4** Given `(READY, MAINTENANCE, Runtime)`, then `IllegalTransition`; `(OFFLINE, MAINTENANCE, Runtime)` is `Ok`.
- **AC-5** Given health `BUSY`, then the warm-up state is `READY`; given `FAULT`, acquisition is refused.

## Design

- **Registry commands:** none; library.
- **Table (allowed; `D` device, `R` runtime):** OFFLINE->CONNECTING (R); CONNECTING->INITIALIZING (D); CONNECTING->OFFLINE (R); INITIALIZING->WARMING_UP (D); INITIALIZING->READY (D); WARMING_UP->READY (D); READY->BUSY (D); BUSY->READY (D); any of {CONNECTING, INITIALIZING, WARMING_UP, READY, BUSY}->FAULT (D or R); any of {INITIALIZING, WARMING_UP, READY, BUSY, FAULT, MAINTENANCE}->SAFETY_STOPPED (D); SAFETY_STOPPED->INITIALIZING (D); any of {OFFLINE, CONNECTING, INITIALIZING, WARMING_UP, READY, FAULT}->MAINTENANCE (D); OFFLINE->MAINTENANCE (R); MAINTENANCE->OFFLINE (D or R); any of {CONNECTING, INITIALIZING, WARMING_UP, READY, BUSY}->INTERRUPTED (R); INTERRUPTED->CONNECTING (R); INTERRUPTED->OFFLINE (R); FAULT->OFFLINE (R); any of {READY, FAULT}->OFFLINE (R on close).
- **Data structures:** `HealthState`, `Actor { Device, Runtime }`, a static sorted table.
- **Errors:** `IllegalTransition { from, to, by }`.
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-hal-idl` (module `base::health`), layer L0.

## Test fixtures

`fixtures/hal/idl/base/health/table.json` generated from the Design table and reviewed (CC-BY-4.0).

## Build prompt

> In crate `fid-hal-idl` (L0), module `base::health`, define `HealthState` (OFFLINE, CONNECTING, INITIALIZING, WARMING_UP, READY, BUSY, FAULT, SAFETY_STOPPED, MAINTENANCE, INTERRUPTED), `Actor { Device, Runtime }`, and a closed static transition table exactly as in the spec's Design; `check(from, to, by) -> Result<(), IllegalTransition>`. Rules to test: no runtime exit from SAFETY_STOPPED (only device-reported SAFETY_STOPPED -> INITIALIZING); MAINTENANCE entered by the device from any state except BUSY and SAFETY_STOPPED, by the runtime only from OFFLINE, exited only to OFFLINE; INTERRUPTED entered only by the runtime and left only to CONNECTING or OFFLINE. Provide `warmup_state(health)` for stamps and `acquisition_allowed(health)` (only READY or BUSY, and WARMING_UP if the package allows). No panic, unwrap, expect or unsafe (CONSTITUTION §8). Tests: AC-1..AC-5 with the exhaustive triple test.

## Open questions

- Whether some packages may acquire during `WARMING_UP` (with the stamp saying so) is left to each capability package; the default here refuses.

## Children

_Leaf: none._
