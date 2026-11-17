---
id: CORE.STO.LAYOUT
title: Project directory layout, locking and cross-platform file behaviour
level: 3
parent: CORE.STO
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-CORE.STO-012, REQ-CORE.STO-013]
covers: []
depends_on: []
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# CORE.STO.LAYOUT: Project directory layout, locking and cross-platform file behaviour

## Purpose

Fixes where a project's bytes live on disk and how they are protected: the on-disk form of a project (folder or single-file archive), the names and roles of its parts (database, blob store, cache, lock), the per-project exclusive write lock, and identical behaviour on Windows, Linux and macOS for long paths and case-only name differences. It also publishes the storage-writer allowlist that the crate-graph lint consumes.

## Scope

**In:**
- Project on-disk form and the decision folder vs archive (made at leaf level, see Open questions)
- Fixed relative paths of database, blob store, cache and lock inside a project
- Exclusive write lock per project, stale-lock detection, typed `locked` error
- Path handling: long paths (at least 1,024 characters), case sensitivity, Unicode normalisation, reserved names on Windows
- The storage-writer crate allowlist (input to `CORE.BUS.LINT`)

**Out** (and which node owns it instead):
- Database schema and transactions (owned by `CORE.STO.SQL`)
- Blob addressing and chunking (owned by `CORE.STO.CAS`)
- Approved import and export roots for agents (policy owned by `CORE.BUS.POL`; configured through `CORE.STO.SET`)

## Requirements

- **REQ-CORE.STO.LAYOUT-001** The storage layer shall place a project's database, blob store and lock file at fixed relative paths inside the project location, and the cache outside it in a per-user cache directory.
- **REQ-CORE.STO.LAYOUT-002** The storage layer shall hold an exclusive write lock per project while a process has it open for writing.
- **REQ-CORE.STO.LAYOUT-003** If a second process opens the same project for writing, then it shall get a typed `locked` error naming the holder's process ID and host.
- **REQ-CORE.STO.LAYOUT-004** If the lock holder's process no longer exists on the same host, then the storage layer shall treat the lock as stale and let a new writer take it after recording a warning.
- **REQ-CORE.STO.LAYOUT-005** The storage layer shall behave identically on Windows, Linux and macOS for project paths up to 1,024 characters.
- **REQ-CORE.STO.LAYOUT-006** The storage layer shall never create two files inside a project whose names differ only in case or Unicode normalisation.
- **REQ-CORE.STO.LAYOUT-007** The storage layer shall publish the list of crates allowed to write project files as a machine-readable allowlist.

## Interfaces

- **Offers:** `ProjectLocation::open(path, mode) -> Result<Locked, StoreError>`; the layout specification; `storage-writers` allowlist file.
- **Consumes:** nothing.

## Decomposition hints

FORM (folder vs archive and fixed paths), LOCK (exclusive lock and stale detection), PATHS (cross-platform path rules and tests), LIST (storage-writer allowlist).

## Open questions

- Single-file archive vs folder affects sharing and cloud-sync tools. The FORM leaf decides unless maintainers want it as a product decision; if they do, open a decision then.
- Network file systems (SMB shares) and SQLite WAL mode do not mix reliably [unverified]; FORM should state whether projects on network shares are supported.

## Children

_None yet: status `open`._
