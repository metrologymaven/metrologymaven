---
id: CORE.STO.SQL.SCHEMA
title: Database schema, append-only triggers and forward migrations
level: 4
parent: CORE.STO.SQL
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-store]
built_pr: 191
satisfies: [REQ-CORE.STO.SQL-001, REQ-CORE.STO.SQL-006]
covers: []
depends_on: [CORE.STO.LAYOUT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.STO.SQL.SCHEMA: Database schema, append-only triggers and forward migrations

## Purpose

The tables of `project.db`, which of them are append-only, the triggers that make append-only a property of the database rather than of our code, the schema version, and forward-only migrations applied in one transaction. Entry semantics (what a row means, how it is hashed) belong to `CORE.JRN`; this leaf only stores rows and protects them.

## Scope

**In:** DDL for tables `entries`, `entry_blobs`, `heads`, `snapshots`, `checkpoints`, `anchors`, `idempotency`, `settings_current`, `schema_meta`; append-only triggers; indexes; WAL mode and pragmas set at open; the migration runner and the migration list format.
**Out:** transactions and durability (`CORE.STO.SQL.TXN`); entry content (`CORE.JRN.ENTRY`); snapshot policy (`CORE.RPL.RECON.SNAP`); settings semantics (`CORE.STO.SET`).

## Requirements

- **REQ-CORE.STO.SQL.SCHEMA-001** The tables `entries`, `entry_blobs`, `snapshots`, `checkpoints` and `anchors` shall be append-only: a `BEFORE UPDATE` and a `BEFORE DELETE` trigger on each shall abort the statement with the message `append_only`.
- **REQ-CORE.STO.SQL.SCHEMA-002** The triggers shall reject UPDATE and DELETE issued through a raw SQLite connection that bypasses the storage API.
- **REQ-CORE.STO.SQL.SCHEMA-003** `heads`, `idempotency` and `settings_current` shall be the only mutable tables, and each shall be rebuildable from the append-only tables.
- **REQ-CORE.STO.SQL.SCHEMA-004** The database shall record its schema version in `schema_meta`, and when a project with an older version is opened for writing, the runner shall apply every later migration in order inside one transaction and record the new version in the same transaction.
- **REQ-CORE.STO.SQL.SCHEMA-005** If the database's schema version is newer than the build supports, then a write open shall fail with `schema_too_new`, and the runner shall never apply a backward migration.
- **REQ-CORE.STO.SQL.SCHEMA-006** If a migration fails, then the transaction shall roll back and the database shall remain at its previous version, byte-for-byte readable as before.
- **REQ-CORE.STO.SQL.SCHEMA-007** At every open the store shall set WAL journal mode and enable foreign keys, and if WAL mode cannot be set, then the write open shall fail with `wal_unavailable`. A read open is read-only and shall leave the journal mode unchanged; it enables foreign keys like every open.

## Acceptance criteria

- **AC-1** Given a new project, when created, then `schema_meta` holds version 1 and the append-only tables carry their two triggers each.
- **AC-2** (error path) Given a raw `rusqlite`-level connection to `project.db`, when it runs `UPDATE entries SET ...` or `DELETE FROM snapshots`, then each statement fails with `append_only` and the row count is unchanged.
- **AC-3** Given fixture `fixtures/core/sto/sql/schema/v1.db` and a test build with a v2 migration adding a column, when opened for writing, then the version becomes 2 and every v1 row is intact.
- **AC-4** (error path) Given a v2 migration with an injected failure in its second statement, then the database stays at version 1 and its file hash after reopening read-only matches before.
- **AC-5** (error path) Given a database at version 99, then a write open fails with `schema_too_new`.

## Design

- **Registry command(s):** none; library.
- **Data structures:** `Migration { from: u32, to: u32, sql: &'static str }` list compiled into the binary; `SchemaVersion(u32)`.
- **Algorithm:** at open: refuse symbolic-link or non-regular `project.db`, `-wal`, `-shm`, `-journal` and a non-empty `-journal`; open with `SQLITE_OPEN_NOFOLLOW` below the canonical root; harden the connection (`SQLITE_DBCONFIG_DEFENSIVE` on, `trusted_schema` off, `cell_size_check` on, `foreign_keys` on, ATTACH unable to create or write, checkpoint-on-close off until checked; extension loading stays off, since `rusqlite` is built without `load_extension`); read version (`schema_meta` must hold exactly 1..=version); refuse a newer version (`schema_too_new`); compare `sqlite_master` (type, name, table, SQL) with the objects the DDL produces for that version (`schema_invalid` on any missing, changed or extra object); only then set `journal_mode=WAL` (write open) and run migrations in a single `BEGIN IMMEDIATE` transaction, re-checking the result before commit. Trigger form: `CREATE TRIGGER <t>_no_update BEFORE UPDATE ON <t> BEGIN SELECT RAISE(ABORT, 'append_only'); END;` and likewise for DELETE.
- **Errors:** `append_only` (from SQLite), `schema_too_new`, `migration_failed`, `wal_unavailable`, `schema_invalid`.
- **Boundary:** the triggers stop data changes (UPDATE, DELETE, replacing INSERT) only. `DROP TRIGGER`, `ALTER`/`DROP TABLE`, `writable_schema` edits, files produced by `VACUUM INTO` or `ATTACH`, a crafted `-wal` and raw byte edits are not stopped by triggers. Schema-level tampering is caught at every open by the schema check (`schema_invalid`, before anything is written), and tampering with row contents by the `CORE.JRN` hash chain.
- **Determinism:** D0 (DDL fixed in source).
- **Performance budget:** open with no migration in < 20 ms [unverified until measured].
- **Crate:** `fid-store` (module `schema`); the SQLite driver crate is on the D-001 allowlist [unverified which binding].

## Test fixtures

`fixtures/core/sto/sql/schema/v1.db` generated by a script from the v1 DDL plus 100 seeded rows; CC-BY-4.0.

## Build prompt

> In crate `fid-store`, add module `schema`: v1 DDL for `entries`, `entry_blobs`, `heads`, `snapshots`, `checkpoints`, `anchors`, `idempotency`, `settings_current`, `schema_meta`; `BEFORE UPDATE` and `BEFORE DELETE` triggers raising `append_only` on `entries`, `entry_blobs`, `snapshots`, `checkpoints`, `anchors`; WAL mode and foreign keys at every open (`wal_unavailable` otherwise). Implement a forward-only migration runner that applies all pending migrations and the version bump in one `BEGIN IMMEDIATE` transaction, rolls back on any failure, and refuses newer schemas with `schema_too_new`. Tests AC-1..AC-5 including a raw-connection test that bypasses the store API. Constraints: CONSTITUTION §6 (append-only journal), §8 (no panic, no `unsafe`), REQ-CORE.STO-001.

## Open questions

- The `snapshots` table stores `SnapshotTaken` records for `CORE.RPL.RECON.SNAP`; `settings_current` holds project-scope values for `CORE.STO.SET`.
- Path length: `CORE.STO.LAYOUT` supports project roots of up to 1,024 characters, but SQLite's Unix VFS accepts a database path of at most about 512 bytes [unverified: SQLite `MAX_PATHNAME`]. On Linux and macOS a write or read open whose `project.db` path is longer than 511 bytes fails with `path_too_long` before SQLite is called, so projects with roots longer than about 500 bytes cannot open their database there. Windows is not affected (the `win32-longpath` VFS with a `\\?\` path). Options: build SQLite with a larger `SQLITE_MAX_PATHLEN`, open through a short alias of the root directory handle, or lower LAYOUT's limit on Unix.

## Children

_Leaf: none._
