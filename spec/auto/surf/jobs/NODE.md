---
id: AUTO.SURF.JOBS
title: Progress, cancellation and honest timeouts for surfaces
level: 3
parent: AUTO.SURF
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.SURF-006, REQ-AUTO.SURF-007]
covers: []
depends_on: [CORE.JOB, CORE.BUS, AUTO.SURF.ENV]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.SURF.JOBS: Progress, cancellation and honest timeouts for surfaces

## Purpose

A surface-side invocation tracker that every surface uses to submit a command, watch its CORE job, request cancellation and, when it stops waiting, classify the result honestly as `not_run` (the bus never accepted it, safe to resend) or `no_reply` (it may have committed). It implements the Craft lesson that timeouts must say whether the command ran.

## Scope

**In:**
- `Invocation` state machine: created → sent → accepted → finished, with timeout and disconnect transitions
- Idempotency key generation per invocation, so a resend after `no_reply` cannot commit twice
- Progress subscription and cancel request over CORE jobs
- Mapping of final states to the shared envelope outcomes

**Out:**
- Job scheduling and cancellation semantics (`CORE.JOB`)
- Surface-specific rendering of progress (`AUTO.CLI.JOB`, `AUTO.MCP.SESS`, `AUTO.CTRL.CLIENT`)

## Requirements

- **REQ-AUTO.SURF.JOBS-001** The tracker shall attach a fresh idempotency key to every invocation and reuse it unchanged on any resend of that invocation.
- **REQ-AUTO.SURF.JOBS-002** The tracker shall mark an invocation `accepted` only on an explicit bus acceptance message carrying the job ID.
- **REQ-AUTO.SURF.JOBS-003** If the wait ends before acceptance, then the tracker shall report `not_run`; if it ends after acceptance and before a final job state, then it shall report `no_reply` with the job ID.
- **REQ-AUTO.SURF.JOBS-004** While an accepted invocation runs, the tracker shall deliver every progress update the job publishes (fraction and stage label) to its subscriber in order.
- **REQ-AUTO.SURF.JOBS-005** When the subscriber requests cancellation of an accepted invocation, the tracker shall send `job.cancel_job@1` for its job and report the job's final state.
- **REQ-AUTO.SURF.JOBS-006** When a resend after `no_reply` returns the original result through the bus idempotency rule, the tracker shall report that result and not a second commit.

## Acceptance criteria

- **AC-1** Given fixture `FakeBus` configured to drop the acceptance message, when an invocation waits 200 ms and gives up, then the outcome is `not_run`.
- **AC-2** Given `FakeBus` that accepts (job `J1`) and then never finishes, when the wait ends, then the outcome is `no_reply` with `job_id = J1`.
- **AC-3** Given the AC-2 state, when the invocation is resent and `FakeBus` returns the committed result for the same idempotency key, then the envelope is `committed` with the original journal entry hash and `FakeBus` records exactly one commit.
- **AC-4** Given `FakeBus` publishing progress 0.1, 0.5, 1.0, then the subscriber receives exactly those three updates in order.
- **AC-5** Given an accepted long job, when cancel is requested, then `job.cancel_job@1` is sent once and the outcome is `cancelled`.
- **AC-6** (property) Over 10,000 random interleavings of accept, progress, finish, drop and timeout from `FakeBus` (seeded), the tracker never reports `not_run` after an acceptance was received.

## Design

- **Registry command(s):** uses `job.show_status@1` (R0), `job.wait_job@1` (R0), `job.cancel_job@1` (W1) from `CORE.JOB`.
- **Data structures:** `Invocation { id, idempotency_key: Uuid (v7), command, state: Created|Sent|Accepted{job}|Finished(Envelope) }`; `trait BusTransport { send, recv }` so in-process, control-channel and host-protocol transports share the tracker.
- **Algorithm:** explicit state machine; timeouts from the caller's deadline; classification is a pure function of the state at the deadline.
- **Errors:** `transport_closed`, `protocol_violation` (finish before accept), both mapped to envelope outcomes.
- **Determinism:** N. Outcomes depend on wall-clock timing; the classification function itself is pure and tested exhaustively.
- **Performance budget:** < 50 µs tracker overhead per invocation [unverified until measured].
- **Crate:** `fid-surface`

## Test fixtures

`FakeBus` test double in `fid-surface` tests (scripted accept, progress, finish, drop); seeded interleaving generator for AC-6.

## Build prompt

> In crate `fid-surface`, implement an invocation tracker over a `BusTransport` trait. Each invocation gets a UUIDv7 idempotency key reused on resend. States: Created → Sent → Accepted{job_id} → Finished. Only an explicit bus acceptance message moves to Accepted. On deadline: before Accepted → envelope outcome `not_run`; Accepted but not finished → `no_reply` with job_id. Forward job progress (fraction, stage label) to a subscriber in order. Cancel sends `job.cancel_job@1` for the job and reports the final state (`cancelled`). A resend after `no_reply` reuses the key and reports the bus's original result. Acceptance: the six cases with a `FakeBus` double, including a seeded property test over 10,000 interleavings proving `not_run` is never reported after acceptance. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; never report `no_reply` as `not_run`; envelope types from `AUTO.SURF.ENV`.

## Open questions

- REQ-AUTO.SURF-006 speaks of any command running longer than 1 s; CORE.JOB gives progress only for commands declared long-running. Commands not declared long-running report stage `running` without a fraction. If that is not enough, `CORE.JOB` would need to wrap every command.

## Children

_Leaf: none._
