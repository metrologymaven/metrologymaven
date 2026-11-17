---
id: CORE.STO.SQL
title: Project database, append-only enforcement and atomic durable commits
level: 3
parent: CORE.STO
kind: module
status: open
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
updated: 2026-11-16
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

- **Offers:** `Store::transaction() -> Txn`, append-only table API, schema version query.
- **Consumes:** `CORE.STO.LAYOUT` (database path, lock), `CORE.STO.CAS` (blob put and flush).

## Decomposition hints

SCHEMA (tables, triggers, migrations), TXN (transaction API and ordering of blob and row writes), CRASH (kill and power-loss harness).

## Open questions

- Power-loss testing needs a fault-injecting file-system layer [unverified which tool]; CRASH should pick one available on Linux CI and document the gap on Windows and macOS.

## Children

_None yet: status `open`._
