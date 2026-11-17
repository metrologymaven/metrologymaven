---
id: CORE.STO
title: Local storage, crash-safe writes and settings
level: 2
parent: CORE
kind: subsystem
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-CORE-008, REQ-CORE-009]
covers: []
depends_on: []
cli: [fid settings *]
mcp: [settings_*]
journal: [settings.*]
updated: 2026-11-16
---

# CORE.STO: Local storage, crash-safe writes and settings

## Purpose

Where bytes live, on the user's machine only. STO provides the per-project SQLite database (append-only journal tables, indexes, snapshots), the content-addressed blob store for large data (point clouds, meshes, video segments, transcripts), atomic and crash-safe write primitives, the project directory layout, and the settings store (user, machine and project scopes) including the privacy defaults: no account, no network, telemetry off. It is separate so that exactly one crate touches the disk and every other subsystem stays storage-agnostic.

## Scope

**In:**
- Project directory layout (one folder or archive per project; exact form fixed at leaf level)
- Per-project SQLite in WAL mode: append-only tables enforced by the database (no UPDATE or DELETE on journal rows)
- Content-addressed blob store keyed by SHA-256, with chunking for large objects and a chunk manifest digest naming the whole object
- Deduplication and blob integrity checks on read
- Atomic transactions spanning journal row plus blob references
- Crash safety: power loss or process kill never leaves a half-written committed entry
- Cache store for snapshots and memoised results, evictable at any time
- Settings: scopes (user, machine, project), typed schema, defaults; settings changes as commands
- Privacy defaults: no account, no network access by default, telemetry off unless opted in; an opt-in setting that is visible and revocable
- Cross-platform file-system behaviour on Windows, Linux and macOS (paths, locking, long paths, case sensitivity)
- Storage lint input: the list of crates allowed to write project files

**Out** (and which node owns it instead):
- Journal entry semantics, hashing and chain (owned by `CORE.JRN`)
- Snapshot content and reconstruction logic (owned by `CORE.RPL`)
- File formats for exchange (owned by `IO`)
- Out-of-core point-cloud chunk formats and LOD (owned by `GEO`)
- Installers and managed enterprise configuration (owned by `PLAT` and `ECO`)
- Telemetry collection itself, if any is ever built (owned by `OUTR` adoption metrics; STO only owns the off-by-default switch)

## Requirements

- **REQ-CORE.STO-001** The storage layer shall reject any UPDATE or DELETE on journal entry rows at the database level, not only in application code.
- **REQ-CORE.STO-002** When a commit returns success, the journal row and every blob it references shall be durable (flushed to stable storage).
- **REQ-CORE.STO-003** If the process is killed at any instant during a commit, then on reopen the store shall contain either the whole commit or none of it.
- **REQ-CORE.STO-004** The blob store shall address every blob by the SHA-256 of its content and shall store identical content once.
- **REQ-CORE.STO-005** If a blob read returns bytes whose SHA-256 differs from its address, then the storage layer shall return a typed integrity error and shall not return the bytes.
- **REQ-CORE.STO-006** The blob store shall support objects of at least 50 GB by chunking, and a digest of the chunk manifest shall name the whole object [unverified that 50 GB covers the largest expected scans].
- **REQ-CORE.STO-007** The core shall run, create, open, edit and save projects with no network interface available and no user account.
- **REQ-CORE.STO-008** The telemetry setting shall default to off, and no telemetry shall be sent unless a human principal has set it on.
- **REQ-CORE.STO-009** Every settings change that affects document, record or configuration state shall be a registry command executed through the bus.
- **REQ-CORE.STO-010** Each setting shall have a typed schema, a default and a scope (user, machine or project); if a stored value fails its schema, then the storage layer shall use the default and report a warning.
- **REQ-CORE.STO-011** Deleting the cache store shall not change any document state; every cached item shall be re-derivable.
- **REQ-CORE.STO-012** The storage layer shall behave identically on Windows, Linux and macOS for paths up to 1,024 characters and for file names differing only in case.
- **REQ-CORE.STO-013** The storage layer shall hold an exclusive write lock per project, and if a second process opens the same project for writing, then it shall get a typed locked error.

## Interfaces

- **Offers:** `Store::transaction()`, append-only table API, `BlobStore::put/get/has`, `Cache`, `Settings::get/schema`; the project directory layout spec.
- **Commands (illustrative):** `settings.set_value@1` (W2 for project scope; A0 for safety-relevant machine settings declared by their owners), `settings.list_values@1` (R0), `settings.reset_value@1` (W2). CLI `fid settings *`; MCP `settings_*` for non-A0 settings only.
- **Consumes:** nothing inside CORE.

## Decomposition hints

LAYOUT (project directory and lock), SQL (SQLite schema and append-only enforcement), CAS (blob store, chunking, integrity), TXN (atomic commit and crash safety), CACHE (snapshots and memo store), SET (settings model and commands), PRIV (offline and telemetry defaults).

## Open questions

- Single-file project archive vs folder: affects sharing and cloud sync tools. Decide at leaf level unless maintainers want it as a product decision.
- Chunk encoding for point data (Arrow IPC plus zstd in the concept docs) must be reconciled with GEO's out-of-core format; STO stores opaque chunks only.

## Split rationale

The dividing principle is **one child per kind of persistence**: LAYOUT (where things are and who may write), SQL (the database and atomic durable commits), CAS (large content-addressed data), CACHE (disposable derived data), SET (typed configuration), PRIV (the offline and telemetry guarantees). This follows the parent's hints, with TXN folded into SQL because atomicity is a property of the database transaction that also orders blob flushes. CACHE and PRIV are small and fully specified, so they are leaves.

Alternatives considered:
- **TXN as its own child.** Rejected: the transaction and the append-only schema are one SQLite unit; splitting would put REQ-002/003 across two children that share one connection.
- **PRIV inside SET.** Rejected: the offline guarantee is a cross-cutting test and the consent rules differ from ordinary settings (W3, human-only, description-bound).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| CORE.STO.LAYOUT | Project directory layout, locking and cross-platform file behaviour | 012 013 | none |
| CORE.STO.SQL | Project database, append-only enforcement and atomic durable commits | 001 002 003 | none |
| CORE.STO.CAS | Content-addressed blob store with chunking and integrity | 004 005 006 | none |
| CORE.STO.CACHE | Evictable cache store for snapshots and memoised results (leaf) | 011 | none |
| CORE.STO.SET | Settings model, scopes and settings commands | 009 010 | none |
| CORE.STO.PRIV | Offline operation and telemetry consent (leaf) | 007 008 | none |
