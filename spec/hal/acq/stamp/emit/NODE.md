---
id: HAL.ACQ.STAMP.EMIT
title: Batch writer - raw storage, stamp validation and journaling
level: 4
parent: HAL.ACQ.STAMP
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-HAL.ACQ.STAMP-001, REQ-HAL.ACQ.STAMP-003, REQ-HAL.ACQ.STAMP-004, REQ-HAL.ACQ.STAMP-005]
covers: []
depends_on: [HAL.ACQ.STAMP.ASSEMBLE, HAL.IDL.STAMP, CORE.STO.CAS, CORE.JRN.ENTRY, CORE.BUS.PIPE]
side_effect: W2
determinism: D0
cli: []
mcp: []
journal: [device.acquisition_batch, device.batch_correction]
updated: 2026-11-17
---

# HAL.ACQ.STAMP.EMIT: Batch writer - raw storage, stamp validation and journaling

## Purpose

The single writer of `device.acquisition_batch` events. For each batch it stores the raw payload in the content-addressed store, checks that the stamp's payload digest matches, validates the stamp against `stamp/0`, and only then journals the event inside the calling command's transaction. It also owns the append-only rule: a journaled stamp is never changed, and a later correction can never clear `simulated`.

## Scope

**In:**
- `BatchWriter::write(assembled, payload, txn) -> WriteOutcome { Journaled(BatchRef) | Rejected(Gap, DeviceEvent) }`
- Store-then-journal ordering and the payload-digest equality check
- Validation against the `stamp/0` and `acquisition-batch/0` validators before journaling
- The sealed event type: no other module can construct a `device.acquisition_batch` payload
- `BatchWriter::correct(batch_ref, correction, txn)` emitting `device.batch_correction` events that reference the batch, with the indelibility rules
- `BatchRef` observation handles for consumers (`HAL.LIVE`, `LVM`, `SCAN`, `CMM`)

**Out:**
- Building the stamp (owned by `HAL.ACQ.STAMP.ASSEMBLE`)
- Recording gaps on a stream (owned by `HAL.ACQ.INGEST.SEQ` and `HAL.ACQ.INGEST.SINK`, which receive the rejection gap)
- Garbage collection of unreferenced store objects (owned by `CORE.STO`)
- Any registry command that requests a correction (none exists yet; a future one must be `human_only`)

## Requirements

- **REQ-HAL.ACQ.STAMP.EMIT-001** The writer shall be the only code path that can construct and journal a `device.acquisition_batch` event; the event payload type shall be constructible only inside this module.
- **REQ-HAL.ACQ.STAMP.EMIT-002** The writer shall store the raw payload bytes in the content-addressed store before journaling, and if the stored object's digest differs from the stamp's payload digest, then it shall reject the batch with `payload_digest_mismatch`.
- **REQ-HAL.ACQ.STAMP.EMIT-003** If the stamp or envelope fails `stamp/0` or `acquisition-batch/0` validation, then the writer shall not journal the batch, shall return a gap with reason `REJECTED` for its sequence range, and shall raise a `device.event` naming the JSON pointer of the failing field.
- **REQ-HAL.ACQ.STAMP.EMIT-004** The writer shall offer no operation that modifies a journaled stamp; a correction shall be a new `device.batch_correction` event that references the batch.
- **REQ-HAL.ACQ.STAMP.EMIT-005** If a correction would set `simulated` from true to false, raise trust, or change the payload digest, then the writer shall refuse it with `stamp_correction_forbidden`.
- **REQ-HAL.ACQ.STAMP.EMIT-006** If journaling fails after the payload was stored, then the writer shall journal nothing for that batch and leave the stored object unreferenced, so a partial batch is never visible.

## Acceptance criteria

- **AC-1** Given the stamp and payload of `fixtures/hal/acq/stamp/emit/linescan_ok/`, when written in a test transaction, then the store holds the payload under the stamp's digest and exactly one `device.acquisition_batch` event is journaled.
- **AC-2** (error path) Given a stamp whose payload digest is one bit off, when written, then the outcome is `Rejected` with `payload_digest_mismatch` and nothing is journaled.
- **AC-3** (error path) Given `fixtures/hal/idl/stamp/missing_simulated.json` as the stamp, when written, then the outcome is `Rejected` with gap reason `REJECTED` and a `device.event` naming `/simulated`.
- **AC-4** (error path) Given a journaled simulated batch and a correction setting `simulated: false`, when `correct` runs, then it fails with `stamp_correction_forbidden` and journals nothing.
- **AC-5** Given a correction that adds a calibration reference, when `correct` runs, then a `device.batch_correction` event references the batch and the original event is unchanged.
- **AC-6** Given a journal stub that fails on append, when written, then no event exists and the stored object has no reference.
- **AC-7** Given a compile-fail test that constructs the event payload type outside the module, then compilation fails.

## Design

- **Registry commands:** none; the writer runs inside the calling command's transaction (`device.import_file_source@1` W2, `device.start_acquisition@1` D1 stream, I++ measurement commands), so its writes carry that command's class. `side_effect: W2` names the strongest P1a caller.
- **Data structures:** `BatchWriter`, sealed `AcquisitionBatchEvent`, `BatchRef { batch_id, payload_digest, journal_entry_hash }`, `StampCorrection` (additive fields only), `WriteOutcome`.
- **Algorithm:** `cas.put(payload) -> digest`; compare; `validate(envelope)`; `txn.append(event)`; corrections diff-checked against the original for forbidden changes.
- **Errors:** `payload_digest_mismatch`, `stamp_invalid { pointer }`, `stamp_correction_forbidden`, `journal_append_failed`.
- **Determinism:** D0 for event bytes given the same assembled stamp and payload.
- **Performance budget:** store, validate and journal one 8 MB batch in under 5 ms on reference hardware, excluding disk sync [unverified until measured].
- **Crate:** `fid-hal-runtime` (module `stamp::emit`), layer L2.

## Test fixtures

Fixtures under `fixtures/hal/acq/stamp/emit/` plus reuse of `fixtures/hal/idl/stamp/` (CC-BY-4.0); in-memory store and journal stubs.

## Build prompt

> In crate `fid-hal-runtime`, module `stamp::emit`, implement `BatchWriter::write(assembled, payload, txn) -> WriteOutcome`: put the payload in the CORE content-addressed store first; reject `payload_digest_mismatch` if the stored digest differs from the stamp's; validate envelope and stamp with the `fid-hal-idl` v0 validators (on failure return `Rejected(Gap { from, to, REJECTED }, device.event naming the JSON pointer)` and journal nothing); then append exactly one `device.acquisition_batch` event in the caller's bus transaction. Make the event payload type sealed (constructible only in this module; add a compile-fail test). Implement `correct(batch_ref, correction, txn)` emitting `device.batch_correction` referencing the batch; refuse (`stamp_correction_forbidden`) any correction that clears `simulated`, raises trust or changes the payload digest; never modify a journaled event. On journal failure leave the stored object unreferenced. No panic, unwrap, expect or unsafe (CONSTITUTION §8). Tests: AC-1..AC-7.

## Open questions

- Whether `simulated` must also be enforced at the result level by CORE (a type that cannot be cleared) or only by TRUST release checks remains a cross-domain question for `CORE` and `TRUST` (inherited).

## Children

_Leaf: none._
