---
id: CORE.STO.SQL.TXN
title: Transaction API, blob-before-row ordering and durability
level: 4
parent: CORE.STO.SQL
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-store]
satisfies: [REQ-CORE.STO.SQL-002, REQ-CORE.STO.SQL-003, REQ-CORE.STO.SQL-004]
covers: []
depends_on: [CORE.STO.SQL.SCHEMA, CORE.STO.CAS, CORE.STO.LAYOUT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.STO.SQL.TXN: Transaction API, blob-before-row ordering and durability

## Purpose

The one way to commit: `Store::transaction()`. A transaction collects blob puts and row inserts; on commit it makes every referenced blob durable first, then writes the rows in one SQLite transaction with full synchronous durability, and only then returns success. Either the whole commit is on disk or none of it is, whatever instant the process dies.

## Scope

**In:** the `Txn` API (put blob, insert row, update mutable table, commit, abort), ordering of blob flushes before row writes, the referenced-blob check, synchronous and checkpoint settings, commit acknowledgement semantics, single-writer serialisation inside a process.
**Out:** table definitions (`CORE.STO.SQL.SCHEMA`); blob writes and flushes themselves (`CORE.STO.CAS`); the crash harness (`CORE.STO.SQL.CRASH`); entry hashing and chain (`CORE.JRN.CHAIN`).

## Requirements

- **REQ-CORE.STO.SQL.TXN-001** When `Txn::commit` returns success, every row of the transaction and every blob it references shall have been flushed to stable storage.
- **REQ-CORE.STO.SQL.TXN-002** The commit shall complete every durable blob put of the transaction before it begins the SQLite write transaction that inserts the referencing rows.
- **REQ-CORE.STO.SQL.TXN-003** If a row references a blob digest that is neither put in the same transaction nor already present in the store, then the commit shall fail with `dangling_blob` and write nothing.
- **REQ-CORE.STO.SQL.TXN-004** The SQLite connection used for commits shall run with WAL journal mode and `synchronous=FULL` [unverified that FULL is needed rather than NORMAL for durability in WAL mode], and the commit shall return only after SQLite reports the transaction committed.
- **REQ-CORE.STO.SQL.TXN-005** If the process dies at any instant during a commit, then after reopen either all rows of that commit are present or none are; blobs put by an uncommitted transaction may remain but shall be unreferenced.
- **REQ-CORE.STO.SQL.TXN-006** Within one process, commits shall be serialised, and a commit attempted on a store opened read-only shall fail with `read_only`.
- **REQ-CORE.STO.SQL.TXN-007** If any statement or blob flush in a commit fails, then the commit shall roll back, return a typed error and leave no row of the transaction visible.

## Acceptance criteria

- **AC-1** Given a transaction with two blobs and one row referencing both, when committed with a probe file system that logs flush calls, then both blob flushes are logged before the SQLite transaction begins and commit returns after the WAL flush.
- **AC-2** (error path) Given a row referencing `sha256:00..00` that is not in the store, then commit fails with `dangling_blob` and the table row count is unchanged.
- **AC-3** (error path) Given a fault injected on the second blob's flush, then commit fails, no row is visible after reopen, and the first blob is present but unreferenced.
- **AC-4** Given two threads committing 1,000 transactions each, then all 2,000 commits are present in a serial order and none interleave rows.
- **AC-5** (error path) Given a store opened read-only, then commit fails with `read_only`.

## Design

- **Registry command(s):** none; library. Only `fid-store` reaches the SQLite driver (`CORE.BUS.LINT`).
- **Data structures:** `Txn { blobs: Vec<PendingBlob>, rows: Vec<RowOp> }`; `RowOp { Insert { table, values }, Upsert { table, key, values } }` (upsert only on mutable tables); `Committed { rows: u64 }`; `TxnError { dangling_blob, read_only, io, sqlite }`.
- **Algorithm:** `commit`: (1) put each pending blob with `durable: true`; (2) check referenced digests against pending set and `has`; (3) `BEGIN IMMEDIATE`; inserts; `COMMIT` with `synchronous=FULL`; (4) return. A process-wide mutex serialises commits; the OS lock from `CORE.STO.LAYOUT` excludes other processes.
- **Errors:** as listed.
- **Determinism:** D0 for row content; commit order follows call order under the mutex.
- **Performance budget:** a commit of one 1 KiB row and no blob in < 5 ms on reference SSD [unverified until measured].
- **Crate:** `fid-store` (module `txn`).

## Test fixtures

Probe and fault-injecting file-system shim in `fid-store` tests (test-only code); no external data. CC-BY-4.0.

## Build prompt

> In crate `fid-store`, add module `txn` with `Store::transaction() -> Txn`, `Txn::put_blob`, `Txn::insert`, `Txn::upsert` (mutable tables only), `Txn::commit() -> Result<Committed, TxnError>`. Commit order: durable put of every pending blob through `cas` (file and directory flushed), then check that every referenced digest is pending or present (`dangling_blob` otherwise), then one `BEGIN IMMEDIATE ... COMMIT` with WAL and `synchronous=FULL`; return only after SQLite reports commit. Roll back on any failure. Serialise commits with a process-wide mutex; refuse commits on read-only stores. Tests AC-1..AC-5 with a probe and fault-injecting file-system shim. Constraints: CONSTITUTION §6, §8 (no panic, no `unsafe`), REQ-CORE.STO-002/003.

## Open questions

- Whether `synchronous=NORMAL` in WAL mode is durable enough across power loss is [unverified]; FULL is chosen until `CORE.STO.SQL.CRASH` measures both.
- Unreferenced blobs left by aborted commits need garbage collection (see `CORE.STO.CAS` open questions).

## Children

_Leaf: none._
