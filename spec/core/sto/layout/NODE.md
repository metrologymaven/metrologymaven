---
id: CORE.STO.LAYOUT
title: Project directory layout, locking and cross-platform file behaviour
level: 3
parent: CORE.STO
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-store]
built_pr: 57
satisfies: [REQ-CORE.STO-012, REQ-CORE.STO-013]
covers: []
depends_on: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.STO.LAYOUT: Project directory layout, locking and cross-platform file behaviour

## Purpose

Fixes where a project's bytes live on disk and how they are protected. A project is a **folder** with fixed relative paths for its database, blob store, carried kernel artifacts, lock and format marker (`format`); the cache lives outside it in the per-user cache directory. One process at a time may hold a project for writing. Paths behave the same on Windows, Linux and macOS up to 1,024 characters, and the store never creates names inside a project that differ only in case or Unicode normalisation. LAYOUT also publishes the storage-writer allowlist that the crate-graph lint reads.

## Scope

**In:** project form (folder), fixed relative paths, layout format version, per-user cache location, the exclusive write lock with stale-lock handling, path handling (long paths, case, normalisation, reserved names), network file-system policy, the `storage-writers.toml` allowlist.
**Out:** database schema and transactions (`CORE.STO.SQL`); blob addressing and chunking (`CORE.STO.CAS`); approved import and export roots for agents (`CORE.BUS.POL`, configured through `CORE.STO.SET`); single-file sharing of a project (an export or package, owned by `TRUST.PKG` and `IO`).

## Requirements

- **REQ-CORE.STO.LAYOUT-001** A project shall be a folder containing exactly these store-managed entries at fixed relative paths: `format` (layout version and project UUID), `project.db` with its WAL companions, `blobs/`, `kernels/`, `tmp/` and `lock`.
- **REQ-CORE.STO.LAYOUT-002** The cache for a project shall live at `<per-user cache dir>/fiducial/cache/<project uuid>/`, outside the project folder.
- **REQ-CORE.STO.LAYOUT-003** While a process has a project open for writing, it shall hold an exclusive operating-system lock on `lock` and keep the holder's process ID, host name and start time in that file.
- **REQ-CORE.STO.LAYOUT-004** If a second process opens the same project for writing, then it shall get a typed `locked` error naming the holder's process ID and host.
- **REQ-CORE.STO.LAYOUT-005** If the lock file names a process on the same host that no longer exists and the OS lock is free, then the store shall take the lock and record a `stale_lock_taken` warning; it shall never take a lock whose holder is on another host.
- **REQ-CORE.STO.LAYOUT-006** The store shall open, create and write projects whose absolute paths are up to 1,024 characters long on Windows, Linux and macOS.
- **REQ-CORE.STO.LAYOUT-007** Every file or directory name the store creates inside a project shall be lowercase ASCII from a fixed set or a lowercase hex digest, so no two can differ only in case or Unicode normalisation, and no user-supplied text shall become a name inside a project.
- **REQ-CORE.STO.LAYOUT-008** If the project folder is on a network file system, then opening it for writing shall fail with `network_fs_unsupported`, and opening it read-only shall be allowed [unverified that detection is reliable on all three OS].
- **REQ-CORE.STO.LAYOUT-009** If `format` names a layout version newer than the running build supports, then opening for writing shall fail with `layout_too_new`.
- **REQ-CORE.STO.LAYOUT-010** The store shall publish `storage-writers.toml`, a machine-readable list of the crates allowed to write project files, for `CORE.BUS.LINT` to read.

## Acceptance criteria

- **AC-1** Given an empty directory, when a project is created, then the folder holds exactly the entries of REQ-001 and the first line of `format` reads `fid-project-layout@1`.
- **AC-2** Given a project open for writing in process A, when process B opens it for writing, then B gets `locked` with A's PID and host.
- **AC-3** Given fixture `fixtures/core/sto/layout/stale_lock` (a lock file naming a dead PID on this host), when opened for writing, then the open succeeds and one `stale_lock_taken` warning is recorded; with a different host name in the file, the open fails with `locked`.
- **AC-4** On Windows, Linux and macOS CI, given a project path of exactly 1,024 characters, when created, written and reopened, then every step succeeds.
- **AC-5** Given a recursive listing of the project after the fixture session `fixtures/core/sto/layout/session_100`, then every name matches `^[a-z0-9_.-]+$` and no two names are equal after case folding and NFC.
- **AC-6** (error path) Given `format` reading `fid-project-layout@99`, then a write open fails with `layout_too_new`.
- **AC-7** Given `storage-writers.toml`, when `CORE.BUS.LINT` runs on the clean fixture workspace, then it parses the file and passes.

## Design

- **Registry command(s):** none; library with no command of its own (`side_effect: R0` follows the convention of `CORE.STO.CACHE`). Journaled effects belong to the commands that cause its file writes (for example `project.create_project@1` in `CORE.PRJ.CONT`).
- **Data structures:** `ProjectLocation { root, uuid, layout_version }`; `OpenMode { Read, Write }`; `Locked` guard (releases on drop); `LockInfo { pid, host, started }`; `StoreError { locked, network_fs_unsupported, layout_too_new, path_too_long, io }`.
- **Algorithm:** Windows paths are passed with the `\\?\` prefix internally; Linux and macOS use the paths as given. OS lock: `LockFileEx` on Windows, `flock` on Linux and macOS [unverified that `flock` is safe with SQLite's own locking; the lock file is separate from `project.db`]. Network file-system detection: volume type query on Windows, `statfs` file-system type on Linux and macOS [unverified completeness]. Projects use a folder rather than a single-file archive because SQLite WAL needs companion files and folders work with every sync tool's file-level model; sharing is an export.
- **`format` file:** line 1 `fid-project-layout@<n>`, line 2 `uuid=<lowercase hyphenated uuid>`. The UUID names the cache folder and is validated strictly because it becomes a path component.
- **Links:** symbolic links (on Windows also junctions and other reparse points) are followed only on the user's path to the project root; below the root the store refuses them (`symlink_refused`).
- **Lock identity:** on Linux and macOS a `flock` counts only if the locked file is still the file at `lock` (same device and inode); on Windows the lock file is held open without write or delete sharing, so it cannot be replaced while held.
- **Manual recovery:** a record from another host, or a garbled record, is never taken over automatically. `ProjectLocation::force_unlock(path, confirmed_holder)` clears it only when no process holds the OS lock and the caller passes back the exact record shown in the `locked` error; it is meant for a human-confirmed command, never automatic.
- **Errors:** as in Data structures, plus `not_a_project`, `not_empty`, `invalid_name`, `lock_file_invalid`, `symlink_refused` and `lock_mismatch` (added at build, PR #57).
- **Determinism:** D0 for path and name handling.
- **Performance budget:** open with lock in < 50 ms [unverified until measured].
- **Crate:** `fid-store` (module `layout`); `storage-writers.toml` at the workspace root listing `fid-store`.

## Test fixtures

`fixtures/core/sto/layout/` (stale lock files, a session generator); long-path tests generate their paths at run time. CC-BY-4.0.

## Build prompt

> In crate `fid-store`, add module `layout`: `ProjectLocation::create/open(path, mode) -> Result<(ProjectLocation, Option<Locked>), StoreError>`. A project is a folder with `format` (`fid-project-layout@1`, then `uuid=<project uuid>`), `project.db` (+ WAL files), `blobs/`, `kernels/`, `tmp/`, `lock`; the cache goes to `<user cache dir>/fiducial/cache/<project uuid>/`. Writing needs an exclusive OS lock on `lock` (LockFileEx on Windows, flock elsewhere) with PID, host and start time written in the file; a second writer gets `locked` with PID and host; a dead same-host holder is taken over with a `stale_lock_taken` warning; other-host holders are never taken over. Support 1,024-character paths (Windows `\\?\`). Only fixed lowercase ASCII names or hex digests inside a project. Refuse write opens on network file systems (`network_fs_unsupported`) and newer layouts (`layout_too_new`). Publish `storage-writers.toml` for `CORE.BUS.LINT`. Tests AC-1..AC-7 on Windows, Linux and macOS CI. Constraints: CONSTITUTION C7, §8 (no panic, no `unsafe`; all paths hostile).

## Open questions

- **Resolved (folder vs archive):** folder (see Design). Maintainers may still want a single-file project as a product choice; if so, a decision should be opened then.
- **Resolved (network shares):** write opens refused, read-only allowed; whether read-only WAL databases open reliably on SMB is [unverified].
- **Resolved at build (name of the marker file):** REQ-001 named it `FORMAT`, which conflicts with REQ-007 and AC-5 (lowercase names only); it is `format`.
- **Project UUID source of truth:** the UUID lives in line 2 of `format`. A later project table (`CORE.PRJ`, `CORE.STO.SQL`) must read it from there or keep it identical, so that the UUID never has two sources of truth.
- **Host identity:** the lock record's host is the `uname` node name (Linux, macOS) or `COMPUTERNAME` (Windows). Renaming a machine makes its own stale locks look foreign; recovery is the human-confirmed `force_unlock` above. A more stable machine identity (for example a machine UUID) may replace the host name later [unverified that one is readable on all three OS without platform FFI].
- `kernels/` is reserved for kernel artifacts carried with a project (`CORE.RPL.EXEC.VERIFY`, D-020).

## Children

_Leaf: none._
