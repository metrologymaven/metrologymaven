---
id: CORE.JRN.CHAIN.APPEND
title: Atomic durable append, sequence and immutability
level: 4
parent: CORE.JRN.CHAIN
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.JRN.CHAIN-001, REQ-CORE.JRN.CHAIN-002, REQ-CORE.JRN.CHAIN-003, REQ-CORE.JRN.CHAIN-004, REQ-CORE.JRN.CHAIN-009]
covers: []
depends_on: [CORE.JRN.ENTRY, CORE.STO.SQL, CORE.STO.CAS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.JRN.CHAIN.APPEND: Atomic durable append, sequence and immutability

## Purpose

The single write path into the chained streams (Record Journal, and in P1b the Access Ledger). APPEND takes an entry draft, assigns the next sequence number and previous hash for its stream, hashes it, stores header, payload and referenced blobs in one durable transaction, and returns the entry hash only after the data is on disk. The API offers no update or delete, and the database refuses them as well.

## Scope

**In:** `Journal::append(draft) -> EntryHash`; per-stream sequence and previous-hash linking (one physical chain per stream over append order, whatever the branch); exactly one Record Journal entry per committed W1, W2, W3, D1, M1 or S1 command and per non-command entry (session header, principal upgrade, head move); durable commit including blobs; group commit for throughput; the throughput benchmark.
**Out:** entry layout and hashing (`CORE.JRN.ENTRY`); which stream an entry goes to and the unchained Activity Log (`CORE.JRN.STREAM.ALOG`); heads and checkpoints (`CORE.JRN.CHAIN.CKPT`); SQLite files, triggers and fsync policy (`CORE.STO.SQL`); blob storage (`CORE.STO.CAS`).

## Requirements

- **REQ-CORE.JRN.CHAIN.APPEND-001** When the bus commits a W1, W2, W3, D1, M1 or S1 command, the journal shall append exactly one Record Journal entry for it.
- **REQ-CORE.JRN.CHAIN.APPEND-002** Each appended entry shall carry a sequence number one greater than the previous entry of the same stream, starting at 1, and the hash of that previous entry (all zeros for sequence 1).
- **REQ-CORE.JRN.CHAIN.APPEND-003** When `append` returns success, the header, payload and every blob the entry references shall already be durable in one transaction.
- **REQ-CORE.JRN.CHAIN.APPEND-004** If any part of the transaction fails, then `append` shall return a typed `append_failed` error and no part of the entry shall be visible after restart.
- **REQ-CORE.JRN.CHAIN.APPEND-005** The journal API shall offer no operation that modifies or deletes an appended entry, and an update or delete issued directly to the entry tables shall be refused by the database.
- **REQ-CORE.JRN.CHAIN.APPEND-006** If a referenced blob digest is not present in the blob store at append time, then `append` shall return `missing_blob` and append nothing.
- **REQ-CORE.JRN.CHAIN.APPEND-007** The journal shall append at least 1,000 entries per second with durable commit on reference hardware, using group commit of concurrent appends where needed [unverified until measured].

## Acceptance criteria

- **AC-1** Given an empty test project, when 100 W2 test commands commit, then the Record Journal holds sequences 1..100, each `prev` equals the previous entry's hash, and entry 1's `prev` is 64 zeros.
- **AC-2** Given fixture `fixtures/core/jrn/chain/append/crash` (a harness that kills the process at 20 injection points inside `append`), when the project reopens after each kill, then either the entry is fully present and verifies, or it is absent with no partial rows or orphan blob references.
- **AC-3** (error path) Given a direct `UPDATE` and `DELETE` on the entries table through a raw connection, then both are refused.
- **AC-4** (error path) Given a draft referencing an absent blob, then `missing_blob` and the head is unchanged.
- **AC-5** Given benchmark `bench_append` (10,000 W2 entries of 2 KiB, 4 concurrent writers on different branches), when run on reference hardware, then throughput is at least 1,000 entries/s [unverified until measured].

## Design

- **Registry command(s):** none; library API.
- **Data structures:** tables `entries(stream, seq, hash, header_json, branch, logical_parent)` and `payloads(hash, payload_json)`; `AppendRequest { draft, blobs: [Digest] }`.
- **Algorithm:** take the stream lock; read the tail `(seq, hash)`; build and hash the header through `entry`; begin transaction; check blobs present; insert header and payload; commit with the durability level `CORE.STO.SQL` defines as full; release. Group commit batches requests that arrive while a commit is in flight into one transaction, preserving arrival order.
- **Errors:** `append_failed`, `missing_blob`, `schema`.
- **Determinism:** D0 for hashes given the draft; arrival order across concurrent writers is observed and fixed by the sequence numbers.
- **Performance budget:** REQ-007.
- **Crate:** `fid-journal` (module `append`).

## Test fixtures

`fixtures/core/jrn/chain/append/crash`: a crash-injection harness with numbered injection points, run in CI on Linux and Windows; CC-BY-4.0.

## Build prompt

> In crate `fid-journal`, add module `append` with `Journal::append(AppendRequest) -> Result<EntryHash, JournalError>`. Per stream, take a lock, read the tail, assign `seq = tail + 1` (first is 1, `prev` 64 zeros for the first), build and hash the header with module `entry`, then in one `fid-store` transaction check every referenced blob exists (`missing_blob`), insert header and payload, and commit fully durable before returning. Provide no update or delete API; rely on the store's triggers that refuse UPDATE/DELETE on entry tables, and test them. Batch concurrent appends into one transaction (group commit) without reordering. Tests: AC-1..AC-5, including the crash-injection harness and `bench_append`. Constraints: CONSTITUTION §6 (append-only, hash-chained), C4, §8 (no panic, no unsafe, deny unwrap/expect).

## Open questions

- From `CORE.JRN.ENTRY` (PR #54): the entry schema accepts any digest as `prev`, so append must refuse a zero `prev` at sequence > 1 and a non-zero `prev` at sequence 1.

## Children

_Leaf: none._
