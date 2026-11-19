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
built_in: [fid-journal]
built_pr: 229
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
- **Data structures:** ~~tables `entries(stream, seq, hash, header_json, branch, logical_parent)` and `payloads(hash, payload_json)`; `AppendRequest { draft, blobs: [Digest] }`.~~ *Superseded by the pinned build choices below (schema v1 mapping and API): `entries(seq, hash, stream, branch, kind, prev_hash, header)`, no `payloads` table (payload blob in `entry_blobs`), `AppendRequest { draft, blobs, expected_prev, salts }`.*
- **Algorithm:** take the stream lock; read the tail `(seq, hash)`; build and hash the header through `entry`; begin transaction; check blobs present; insert header and payload; commit with the durability level `CORE.STO.SQL` defines as full; release. Group commit batches requests that arrive while a commit is in flight into one transaction, preserving arrival order.
- **Errors:** ~~`append_failed`, `missing_blob`, `schema`.~~ *Superseded by the pinned build choices below, which add `head_moved`, `time_regressed`, `time_ahead`, `chain_invalid` and `input_limit`.*
- **Determinism:** D0 for hashes given the draft; arrival order across concurrent writers is observed and fixed by the sequence numbers.
- **Performance budget:** REQ-007.
- **Crate:** `fid-journal` (module `append`).
- **Pinned build choices (fid-journal `append`, feature `store`):**
  - **Feature gate.** The module sits behind cargo feature `store` (optional dependency `fid-store`), so the entry model stays storage-free for `fid-replay` and other users built for wasm32. `fid-journal` reaches SQLite only through `fid-store` (the storage writer), never through `rusqlite` directly.
  - **API.** `Journal::open(ProjectLocation, Locked, stream) -> Journal` (keeps the write lock; system clock) and `Journal::open_with_clock(..., Arc<dyn Clock>)`; `Journal::append(AppendRequest) -> Result<EntryHash, JournalError>`; `AppendRequest { draft, blobs: [Digest], expected_prev: Option<Digest>, salts: Option<RecordedSalts> }`. The injection seam is trait `Append { fn append(&self, AppendRequest) -> Result<EntryHash, JournalError>; fn tail(&self) -> Result<Option<Tail>, JournalError> }`, `Send + Sync`, implemented for `Journal`, `Arc<T>` and `&T`; `tail` lets the bus build `expected_prev`. No update or delete operation exists.
  - **Schema v1 mapping.** `entries(seq, hash, stream, branch, kind, prev_hash, header)`: `header` is the RFC 8785 header with `hash`; `prev_hash` is always set, to 64 zeros at sequence 1. Schema v1 has no `payloads` table: the payload (salts and committed values, RFC 8785) is a content-addressed blob, referenced from `entry_blobs` with role `payload`; referenced blobs get role `ref`. Schema v1 keys `entries` by `seq` alone, so one project database holds one chained stream, the one the journal is opened for: a draft for another stream is `schema`, a database holding another stream is `chain_invalid`. The Access Ledger (P1b) needs a `(stream, seq)` key first.
  - **Sequence and link.** One chain per stream over append order, whatever the branch. The tail row is verified before every batch: its header hash; its `seq`, `stream`, `branch`, `kind` and `prev_hash` columns against the header; the genesis rule (a zero `prev` exactly at sequence 1, which settles the Open question below); and, above sequence 1, that its `prev` equals the stored hash at sequence - 1 (one indexed lookup, so a forked tail is caught). A tail that fails is `chain_invalid`. Open also checks that sequences are 1..n without gaps.
  - **Time validity.** `time.utc` must be a real calendar instant: month 1-12, a day that exists in that month (Gregorian leap years), hour 0-23, minute 0-59, second 0-59, and second 60 only at 23:59:60 (a leap second, equal to the next day's 00:00:00); otherwise `schema`.
  - **Time ahead.** An entry whose `time.utc` lies more than 300 s after the injected host `Clock` (strictly greater; exactly 300 s is accepted) is refused with `time_ahead`, so one far-future stamp cannot lock the chain against every later entry under the time-order rule. A system clock before the Unix epoch reads as the epoch and refuses every entry (fail closed).
  - **Time order.** An entry whose `time.utc` instant is earlier than the chain tail's is refused with `time_regressed` and nothing is appended; an equal instant is accepted. Instants are compared as instants on the Unix time line (`...:00Z` equals `...:00.000Z` and is earlier than `...:00.5Z`); `mono_ns` is not compared because it restarts with the host. The bus stamps a `principal_upgrade` at confirm time and commits it later (#222), so the guard sits here; such a refusal is the bus's `conflict/head_moved` case.
  - **Concurrency.** Appends are serialised in arrival order. With `expected_prev` set, a request whose expected tail hash (64 zeros for an empty chain) is no longer the tail fails with `head_moved` (the bus catalogue's `conflict/head_moved`) and appends nothing; without it the request links to the tail at its turn.
  - **Group commit.** Requests that arrive while a commit is in flight are taken, at most 256 in arrival order, into the next transaction; each runs in its own SAVEPOINT, so one refusal does not affect the others.
  - **Atomicity and durability.** Per append: tail read inside `BEGIN IMMEDIATE`; referenced blobs checked (`missing_blob`; a chunk manifest needs every chunk) and flushed (`sync_all` on the file, its two fan-out directories and `sha256/`); payload put with `PutOpts::durable`; `entries` and `entry_blobs` inserted; COMMIT. The connection runs WAL with `synchronous=FULL`, `fullfsync=ON` and `checkpoint_fullfsync=ON` (`F_FULLFSYNC` on macOS), set and read back at open. The COMMIT is the only point where an entry becomes visible; every failure before the COMMIT rolls back, leaving at most an unreferenced payload blob, and nothing after a successful COMMIT can fail the call.
  - **Failed COMMIT: outcome unknown.** A COMMIT that reports an error may still have written its commit frame (the WAL fsync can fail after the write), so the batch may reappear after a restart; an `append_failed` from a failed COMMIT therefore does not mean "nothing was appended". Every request of the batch gets `append_failed` naming the unknown outcome, and the journal is poisoned: every later `append` and `tail` returns `append_failed` until the project is reopened, which re-reads the durable tail. Callers resolve the outcome after the reopen with `tail()`, or by retrying with `expected_prev` set to the tail they saw before (`head_moved` then means the entry did land).
  - **Errors.** `append_failed`, `missing_blob`, `schema`, plus `head_moved`, `time_regressed`, `time_ahead`, `chain_invalid` (stored chain does not verify at its tail, is forked there, or holds another stream) and `input_limit` (header over 1 MiB, payload over 8 MiB, more than 4096 referenced blobs).
  - **Bounds.** At most 1024 queued requests (later callers wait), 256 per transaction; no panics (lock poisoning is recovered, sequence overflow is `append_failed`).
  - **Crash harness (AC-2).** 20 numbered points in `CrashPoint`, listed with their hand-labelled outcomes in `fixtures/core/jrn/chain/append/crash/points.json`. The test re-runs its own binary as a child that exits the process (no destructors, no SQLite close) at each point, then reopens and verifies the whole chain and every blob reference. The same points inject in-process failures (`append_failed`, chain at the old head); a refusal at `committed` simulates a COMMIT error after the write and must poison the journal. Point 1, `prechecked`, fires after the request checks and before the request is queued. The hook exists only with feature `test-hooks`.
  - **Benchmark (AC-5).** `bench_append` is an ignored test (10,000 W2 entries of about 2 KiB, 4 writers on different branches); the 1,000 entries/s budget stays [unverified until measured].

## Test fixtures

`fixtures/core/jrn/chain/append/crash`: a crash-injection harness with numbered injection points, run in CI on Linux, Windows and macOS; CC-BY-4.0. `chain.json` (100 W2 entries with expected hashes and payload digests) and `time_order.json` come from the independent stdlib Python reference `make_fixtures.py` next to them (`--check` in CI).

## Build prompt

> In crate `fid-journal`, add module `append` with `Journal::append(AppendRequest) -> Result<EntryHash, JournalError>`. Per stream, take a lock, read the tail, assign `seq = tail + 1` (first is 1, `prev` 64 zeros for the first), build and hash the header with module `entry`, then in one `fid-store` transaction check every referenced blob exists (`missing_blob`), insert header and payload, and commit fully durable before returning. Provide no update or delete API; rely on the store's triggers that refuse UPDATE/DELETE on entry tables, and test them. Batch concurrent appends into one transaction (group commit) without reordering. Tests: AC-1..AC-5, including the crash-injection harness and `bench_append`. Constraints: CONSTITUTION §6 (append-only, hash-chained), C4, §8 (no panic, no unsafe, deny unwrap/expect).

## Open questions

- From `CORE.JRN.ENTRY` (PR #54): the entry schema accepts any digest as `prev`, so append must refuse a zero `prev` at sequence > 1 and a non-zero `prev` at sequence 1. *Settled by the build: append computes `prev`, and the tail check refuses both cases.*
- **Recovery from a past bad stamp.** The time-ahead rule stops new far-future stamps, but a chain may already hold one: written before this rule, under a host clock that was itself wrong, or by another build. Its tail then refuses every honest later entry as `time_regressed`, and the chain cannot be edited. The way out is not settled. Options include a signed `head_move`-like "clock correction" entry that a person confirms and that resets the order baseline, refusing to open such a project until a person acknowledges it, or comparing only within a session. This needs a maintainer decision (and probably `CORE.JRN.ENTRY` and `TRUST` input) before P1.

## Children

_Leaf: none._
