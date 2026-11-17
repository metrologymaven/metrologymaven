---
id: HAL.HOST.TIER1
title: In-process (Tier-1) driver API and registration
level: 3
parent: HAL.HOST
kind: task
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-HAL.HOST-001]
covers: []
depends_on: [HAL.IDL.GEN, HAL.ACQ.SESS, HAL.ACQ.INGEST]
side_effect: D1
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# HAL.HOST.TIER1: In-process (Tier-1) driver API and registration

## Purpose

The Rust API that first-party in-process drivers implement (the I++ client, gauges, environment sensors, later OPC UA), and the static registration that makes them available to the runtime. It gives Tier-1 drivers the same semantics as out-of-process hosts (deadlines, idempotency keys, typed errors, credit-based batches) so a device class can move between tiers without changing the runtime.

## Scope

**In:**
- The `Driver` trait family generated from capability packages (`DeviceDriver`, plus one trait per capability)
- Call context passed to every control call: trace ID, idempotency key, deadline
- Static, compile-time registration of Tier-1 drivers with a manifest entry declaring `tier: 1`
- A per-call deadline watchdog: a call that exceeds its deadline returns `TRANSIENT` or `COMM_LOST` to the runtime, and the driver instance is marked faulted
- Adapter from the trait's batch output to the ingest `BatchSink`

**Out:**
- Out-of-process hosts and their protocol (owned by `HAL.HOST.PROTO` and `HAL.HOST.SUPER`)
- Driver implementations (owned by `HAL.IPP` and `HAL.DRV`)
- Dynamic loading of Rust drivers (not offered: Tier-1 is first-party and compiled in)

## Requirements

- **REQ-HAL.HOST.TIER1-001** Every Tier-1 driver shall be registered at compile time with a manifest entry declaring `tier: 1`, its driver ID, version and the capabilities it implements.
- **REQ-HAL.HOST.TIER1-002** Every control call into a Tier-1 driver shall carry a trace ID, an idempotency key and a deadline, and repeating a call with the same idempotency key shall not repeat its effect.
- **REQ-HAL.HOST.TIER1-003** If a Tier-1 control call exceeds its deadline, then the runtime shall return `COMM_LOST` to the session, mark the driver instance `FAULT`, and keep the engine running.
- **REQ-HAL.HOST.TIER1-004** A Tier-1 driver shall deliver data only through the credit-limited `BatchSink`, and shall have no access to the journal, the content-addressed store or the command bus.
- **REQ-HAL.HOST.TIER1-005** The Tier-1 API shall expose no way to bypass a registry command: every state-changing method shall require the call context created by `HAL.ACQ.SESS` for a journaled command.

## Acceptance criteria

- **AC-1** Given the test driver `fixtures/hal/host/tier1/mock_gauge` (in-process `Gauge` driver), when the runtime opens a session and reads 100 values, then 100 stamped batches are journaled with driver tier 1 in their stamps.
- **AC-2** Given the mock driver configured to sleep 2 s on `read` with a 500 ms deadline, when `read` is called, then the session receives `COMM_LOST` within 600 ms, the instance is `FAULT`, and a second device session in the same engine keeps streaming.
- **AC-3** Given two `set_configuration` calls with the same idempotency key, when both run, then the mock driver records exactly one configuration change.
- **AC-4** (error path) Given a call constructed without a session-issued call context, when compiled, then it fails to compile (type-level guarantee checked by a `trybuild` test).
- **AC-5** Given the registry of Tier-1 drivers, when `device.list_drivers@1` runs, then the mock driver is listed with `tier: 1`.

## Design

- **Registry commands:** none of its own; used by `device.open_session@1` and capability commands (D1). Listing goes through `HAL.HOST.PKG`.
- **Data structures:** `CallCtx { trace_id, idempotency_key, deadline }` constructible only by the session module; `DriverRegistration { id, version, tier: 1, capabilities }` collected with a static registry macro; generated capability traits.
- **Algorithm:** each driver instance runs on its own task; calls are wrapped in a deadline timer; an idempotency cache (bounded, BTreeMap keyed by key) per instance.
- **Errors:** `COMM_LOST`, `TRANSIENT`, plus whatever the driver returns from the closed error set.
- **Determinism:** N (device I/O).
- **Performance budget:** call overhead < 20 µs excluding the driver's own work [unverified until measured].
- **Crate:** `fid-hal-driver` (API) used by `fid-hal-runtime`.

## Test fixtures

`fixtures/hal/host/tier1/mock_gauge`: a project-written in-process mock driver with configurable delays and failures (Apache-2.0 test code).

## Build prompt

> In a new crate `fid-hal-driver`, define the Tier-1 driver API: generated capability traits (from `fid-hal-idl` stubs), `CallCtx { trace_id, idempotency_key, deadline }` that only the session module of `fid-hal-runtime` can construct (private constructor plus sealed token), a compile-time registration macro producing `DriverRegistration { id, version, tier: 1, capabilities }`, and a `BatchSink` adapter that is credit-limited. In `fid-hal-runtime`, run each Tier-1 instance on its own task, enforce deadlines (exceeded → `COMM_LOST` to the session, instance `FAULT`, engine keeps running), and de-duplicate by idempotency key. Drivers get no access to journal, store or bus. No panic, unwrap, expect or unsafe (CONSTITUTION §8). Tests: AC-1..AC-5 using the mock gauge in `fixtures/hal/host/tier1/mock_gauge`, including a `trybuild` compile-fail test for AC-4.

## Open questions

- A Rust driver that loops without yielding cannot be pre-empted on its task; Tier-1 is reserved for first-party code reviewed against this, and anything untrusted runs in Tier 2.

## Children

_Leaf: none._
