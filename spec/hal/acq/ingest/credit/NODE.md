---
id: HAL.ACQ.INGEST.CREDIT
title: Credit ledger for data-plane flow control
level: 4
parent: HAL.ACQ.INGEST
kind: task
status: leaf
phase: P2
size: S
owners: []
claimed_by:
built_in: [fid-hal-runtime]
satisfies: [REQ-HAL.ACQ.INGEST-001]
covers: []
depends_on: [HAL.IDL.STAMP]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# HAL.ACQ.INGEST.CREDIT: Credit ledger for data-plane flow control

## Purpose

The per-stream credit ledger that makes a slow consumer slow the source down instead of losing samples. The runtime grants credits (in bytes and in batches) as downstream stages free capacity; a source may send only while it holds credit. It is a separate leaf because the "no silent drops" rule depends on this accounting being exact, and it can be tested as a pure state machine without any transport.

## Scope

**In:**
- `CreditLedger` state machine per stream: grant, consume, release, exhaust
- Credit units: bytes and batch count, both limits enforced
- The over-credit violation and its gap reason `CREDIT_EXHAUSTED`
- Grant policy: refill to a window as downstream stages acknowledge batches

**Out:**
- Carrying credits on the wire (owned by `HAL.HOST.PROTO`)
- Sequence checks and batch admission rules (owned by `HAL.ACQ.INGEST.SEQ`)
- Wiring the ledger into the batch path (owned by `HAL.ACQ.INGEST.SINK`)

## Requirements

- **REQ-HAL.ACQ.INGEST.CREDIT-001** The ledger shall accept a batch only if its byte size and the batch count both fit within the outstanding credit, and shall deduct both on acceptance.
- **REQ-HAL.ACQ.INGEST.CREDIT-002** If a source sends a batch that exceeds its outstanding credit, then the ledger shall refuse it with `credit_exceeded` and report the covered sequence range as a gap with reason `CREDIT_EXHAUSTED`.
- **REQ-HAL.ACQ.INGEST.CREDIT-003** When a downstream stage acknowledges a batch as stamped or rejected, the ledger shall return its credit, never exceeding the configured window (default 64 MB and 16 batches [unverified until tuned]).
- **REQ-HAL.ACQ.INGEST.CREDIT-004** The ledger shall never grant credit while the stream is stopping or stopped.
- **REQ-HAL.ACQ.INGEST.CREDIT-005** Given the same sequence of grant, consume and acknowledge events, the ledger shall produce the same states and outputs.

## Acceptance criteria

- **AC-1** Given a window of 8 MB and 4 batches and the event script `fixtures/hal/acq/ingest/credit/steady.json`, when replayed through the ledger, then every batch is accepted and outstanding credit never goes negative.
- **AC-2** (error path) Given `fixtures/hal/acq/ingest/credit/overrun.json` (source sends a fifth batch with no credit), when replayed, then that batch is refused with `credit_exceeded` and a gap `{from, to, CREDIT_EXHAUSTED}` covering its sequence range is reported.
- **AC-3** Given acknowledgements that would push credit above the window, when applied, then outstanding credit equals the window exactly.
- **AC-4** Given a stream in `stopping`, when a grant is requested, then no credit is granted.
- **AC-5** Given 10 000 random event scripts from a seeded generator (seed 11), when each is replayed twice, then both runs produce identical state traces (property test).

## Design

- **Registry commands:** none; runs inside `device.start_acquisition@1` (D1), owned by `HAL.ACQ.INGEST.CMDS`.
- **Data structures:** `CreditWindow { bytes: u64, batches: u32 }`, `CreditLedger { window, outstanding, state: Active | Stopping | Stopped }`, `Admit { Accepted | Refused(Gap) }`. `Gap` and `GapReason` come from `fid-hal-idl` (`HAL.IDL.STAMP`).
- **Algorithm:** integer arithmetic only, saturating where the window caps; no clock reads (time is not an input).
- **Errors:** `credit_exceeded`.
- **Determinism:** D0; pure integer state machine.
- **Performance budget:** under 100 ns per admit on reference hardware [unverified until measured].
- **Crate:** `fid-hal-runtime` (module `ingest::credit`), layer L2.

## Test fixtures

Project-authored event scripts under `fixtures/hal/acq/ingest/credit/` (CC-BY-4.0) and a seeded property-test generator.

## Build prompt

> In crate `fid-hal-runtime`, module `ingest::credit`, implement `CreditLedger` with a `CreditWindow { bytes: u64, batches: u32 }` (default 64 MB, 16 batches), states `Active | Stopping | Stopped`, and methods `grant()`, `admit(batch_bytes, seq_range) -> Admit` and `ack(batch_bytes)`. A batch is admitted only if both byte and batch credit suffice; otherwise refuse with `credit_exceeded` and return `Gap { from, to, reason: CREDIT_EXHAUSTED }` (types from `fid-hal-idl`). Acks return credit capped at the window; no grants while stopping or stopped. Integer arithmetic only, no clock reads, no panic, unwrap, expect or unsafe (CONSTITUTION §8). Tests: AC-1..AC-5 with scripts in `fixtures/hal/acq/ingest/credit/` and a seeded property test.

## Open questions

- The default window size is a proposal to be tuned with `HAL.ACQ.INGEST.BENCH` [unverified].

## Children

_Leaf: none._
