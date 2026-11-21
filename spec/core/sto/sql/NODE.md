---
id: CORE.STO.SQL
title: Project database, append-only enforcement and atomic durable commits
level: 3
parent: CORE.STO
kind: module
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-CORE.STO-001, REQ-CORE.STO-002, REQ-CORE.STO-003]
covers: []
depends_on: [CORE.STO.LAYOUT, CORE.STO.CAS]
cli: []
mcp: []
journal: []
updated: 2026-11-21
---

# CORE.STO.SQL: Project database, append-only enforcement and atomic durable commits

## Purpose

The per-project SQLite database in WAL mode and the transaction primitive every commit uses. SQL defines the tables (entries, blob index, heads, snapshots, checkpoints, anchors, idempotency), enforces append-only behaviour in the database itself, and guarantees that a commit (journal row plus blob references) is either wholly durable or wholly absent, whatever instant the process dies.

## Scope

**In:**
- Schema and schema migrations for the project database
- Append-only enforcement by database triggers on journal tables
- `Store::transaction()` spanning journal rows and blob references; blobs written and flushed before the row that references them
- Durability settings (synchronous mode, flush behaviour) per platform
- Crash-safety test harness (kill at random instants, power-loss simulation)

**Out** (and which node owns it instead):
- Entry semantics and hashing (owned by `CORE.JRN`)
- Blob storage (owned by `CORE.STO.CAS`)
- File locations and locks (owned by `CORE.STO.LAYOUT`)

## Requirements

- **REQ-CORE.STO.SQL-001** The project database shall reject any UPDATE or DELETE on journal entry rows by database triggers, so the rejection holds even for code that bypasses the storage API.
- **REQ-CORE.STO.SQL-002** When a commit returns success, the journal row and every blob it references shall have been flushed to stable storage.
- **REQ-CORE.STO.SQL-003** The storage layer shall write and flush every referenced blob before writing the journal row that references it.
- **REQ-CORE.STO.SQL-004** If the process is killed at any instant during a commit, then on reopen the store shall contain either the whole commit or none of it.
- **REQ-CORE.STO.SQL-005** When a crash harness kills the process at 1,000 random instants during commits, then every reopen shall pass the journal chain check and lose no commit that had returned success.
- **REQ-CORE.STO.SQL-006** When a project created by an older schema version is opened for writing, the storage layer shall migrate it forward in one transaction and never migrate backwards.

## Interfaces

- **Offers:** `Store::transaction() -> Txn`, append-only table API, schema version query (SCHEMA, TXN); the crash harness, reused by `CORE.RPL.RECON.FOLD` and `CORE.JRN.CHAIN` tests (CRASH).
- **Consumes:** `CORE.STO.LAYOUT` (database path, lock), `CORE.STO.CAS` (blob put and flush), `CORE.JRN.CANON` (the harness's synthetic chain).

## Decomposition hints

Applied: SCHEMA, TXN, CRASH.

## Open questions

- Power-loss testing needs a fault-injecting file-system layer [unverified which tool]; CRASH picks one available on Linux CI and documents the gap on Windows and macOS.
- The "journal chain check" in REQ-005 cannot call `CORE.JRN.CHAIN` (CHAIN depends on `CORE.STO`, so a dependency would form a cycle). CRASH checks a synthetic chain of its own built with `CORE.JRN.CANON`; `CORE.JRN.CHAIN` runs the same harness with real entries in its own tests.

## Change log

- 2026-11-21 (maintainer decision on D-047, review of #401 and #405): `fid-store`'s SQLite checks are shared with the results catalog (`RPT.CATALOG.STORE.SCHEMA`, crate `fid-catalog`) instead of copied. A dependent gets a connection only through one guarded door, `open_hardened(dir, name, mode, busy)`: it requires an absolute directory and a plain file name, refuses a missing, non-regular or symlinked file and bad companions (`check_companions_of`), opens with `SQLITE_OPEN_NOFOLLOW` and without `SQLITE_OPEN_CREATE` or `SQLITE_OPEN_URI`, applies and reads back every hardening setting (checkpoint-on-close off until `allow_checkpoint` is called after the caller's checks) and sets the busy timeout. The raw open, the hardening step and the `sqlite3_db_config` setter stay private, so no dependent can obtain an unhardened connection (`CORE.BUS.LINT`); a test checks the public items and compile-fail doc tests check the private ones. Also public: `check_companions_of(dir, name)`, `check_root_pages`, `expected_objects`, `verify_schema`, `quick_check`, `set_wal`, and the migration runner `migrate_with` with a `MigrationHooks` trait (how a format checks, reads and records its version). `project.db` opens through the same door and uses the runner through `StoreHooks` (versions in `schema_meta`) with unchanged behaviour and unchanged tests. No REQ or AC changed.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| CORE.STO.SQL.SCHEMA | Database schema, append-only triggers and forward migrations (leaf) | 001 006 | none |
| CORE.STO.SQL.TXN | Transaction API, blob-before-row ordering and durability (leaf) | 002 003 004 | none |
| CORE.STO.SQL.CRASH | Crash and power-loss harness (leaf) | 005 | none |
