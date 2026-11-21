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
updated: 2026-11-21
---

# CORE.STO.LAYOUT: Project directory layout, locking and cross-platform file behaviour

## Purpose

Fixes where a project's bytes live on disk and how they are protected. A project is a **folder** with fixed relative paths for its database, blob store, carried kernel artifacts, lock and format marker (`format`); the cache lives outside it in the per-user cache directory. One process at a time may hold a project for writing. Paths behave the same on Windows, Linux and macOS up to 1,024 characters, and the store never creates names inside a project that differ only in case or Unicode normalisation. LAYOUT also publishes the storage-writer allowlist that the crate-graph lint reads.

## Scope

**In:** project form (folder), fixed relative paths, layout format version, per-user cache location, the exclusive write lock with stale-lock handling, path handling (long paths, case, normalisation, reserved names), network file-system policy, the `storage-writers.toml` allowlist.
**Out:** database schema and transactions (`CORE.STO.SQL`); blob addressing and chunking (`CORE.STO.CAS`); approved import and export roots for agents (`CORE.BUS.POL`, configured through `CORE.STO.SET`); single-file sharing of a project (an export or package, owned by `TRUST.PKG` and `IO`).

## Requirements

- **REQ-CORE.STO.LAYOUT-001** A project shall be a folder containing exactly these store-managed entries at fixed relative paths: `format` (layout version and store-instance UUID), `project.db` with its WAL companions, `blobs/`, `kernels/`, `tmp/` and `lock`.
- **REQ-CORE.STO.LAYOUT-002** The cache for a project shall live at `<per-user cache dir>/fiducial/cache/<store-instance uuid>/`, outside the project folder.
- **REQ-CORE.STO.LAYOUT-003** While a process has a project open for writing, it shall hold an exclusive operating-system lock on `lock` and keep the holder's process ID, host name and start time in that file.
- **REQ-CORE.STO.LAYOUT-004** If a second process opens the same project for writing, then it shall get a typed `locked` error naming the holder's process ID and host.
- **REQ-CORE.STO.LAYOUT-005** If the lock file names a process on the same host that no longer exists and the OS lock is free, then the store shall take the lock and record a `stale_lock_taken` warning; it shall never take a lock whose holder is on another host.
- **REQ-CORE.STO.LAYOUT-006** The store shall open, create and write projects whose absolute paths are up to 1,024 characters long on Windows, Linux and macOS.
- **REQ-CORE.STO.LAYOUT-007** Every file or directory name the store creates inside a project shall be lowercase ASCII from a fixed set or a lowercase hex digest, so no two can differ only in case or Unicode normalisation, and no user-supplied text shall become a name inside a project.
- **REQ-CORE.STO.LAYOUT-008** If the project folder is on a network file system, then opening it for writing shall fail with `network_fs_unsupported`, and opening it read-only shall be allowed [unverified that detection is reliable on all three OS].
- **REQ-CORE.STO.LAYOUT-009** If `format` names a layout version newer than the running build supports, then opening for writing shall fail with `layout_too_new`.
- **REQ-CORE.STO.LAYOUT-010** The store shall publish `storage-writers.toml`, a machine-readable list of the crates allowed to write project files, for `CORE.BUS.LINT` to read.
- **REQ-CORE.STO.LAYOUT-011** When the store creates or opens a project from a `CORE.BUS.POL.FS` folder target, it shall open the folder through the target, refuse with `outside_root` if the opened folder's identity differs from the one the target recorded, check that identity again after opening `project.db` and refuse on a mismatch, and open a target marked read-only only with `OpenMode::Read` and no write of any file.
- **REQ-CORE.STO.LAYOUT-012** When a project is copied, the store shall write the copy into a new temporary sibling folder in the target's parent folder (and so the same root) holding its own `lock`, copy `project.db` only with the SQLite online backup API from a read transaction of the source, copy every CAS object the snapshot references (including every chunk of a chunked object) by re-reading and re-hashing it against its digest, fsync every written file and folder, and create the target only by a no-replace rename of the temporary folder.
- **REQ-CORE.STO.LAYOUT-013** If a project copy fails at any step, including a crash, then no folder shall appear at the target name, the temporary folder shall be removed (at once on an error, or by the next copy into the same parent folder after a crash), and the source project shall be unchanged.

## Acceptance criteria

- **AC-1** Given an empty directory, when a project is created, then the folder holds exactly the entries of REQ-001 and the first line of `format` reads `fid-project-layout@1`.
- **AC-2** Given a project open for writing in process A, when process B opens it for writing, then B gets `locked` with A's PID and host.
- **AC-3** Given fixture `fixtures/core/sto/layout/stale_lock` (a lock file naming a dead PID on this host), when opened for writing, then the open succeeds and one `stale_lock_taken` warning is recorded; with a different host name in the file, the open fails with `locked`.
- **AC-4** On Windows, Linux and macOS CI, given a project path of exactly 1,024 characters, when created, written and reopened, then every step succeeds.
- **AC-5** Given a recursive listing of the project after the fixture session `fixtures/core/sto/layout/session_100`, then every name matches `^[a-z0-9_.-]+$` and no two names are equal after case folding and NFC.
- **AC-6** (error path) Given `format` reading `fid-project-layout@99`, then a write open fails with `layout_too_new`.
- **AC-7** Given `storage-writers.toml`, when `CORE.BUS.LINT` runs on the clean fixture workspace, then it parses the file and passes.
- **AC-8** Given a project with 20 entities and 50 blobs, two of them chunked, open for writing in another thread that keeps committing, when it is copied, then the copy opens, its `project.db` passes `PRAGMA integrity_check`, its entries are a prefix-consistent snapshot of the source (every entry up to one sequence number, none after), and every object, manifest and chunk re-hashes to its name.
- **AC-9** (crash) Given a copy killed by the crash harness of `CORE.STO.SQL.CRASH` at 50 points (seed 11) spread over database backup, blob copy, fsync, handle close and rename, when the parent folder is listed and the next copy runs, then no folder exists at the target name unless the rename completed, in which case it is a complete project that passes AC-8's checks. No temporary folder survives the next copy, and the source passes `journal.verify_chain@1` unchanged.
- **AC-10** (error path) Given a target name that exists, or a blob whose content no longer matches its name, then the copy fails (`not_empty` or `blob_corrupt`), nothing appears at the target and no temporary folder remains; given a folder swapped after the identity check but before `project.db` is opened, or after it, then `outside_root`.

## Design

- **Registry command(s):** none; library with no command of its own (`side_effect: R0` follows the convention of `CORE.STO.CACHE`). Journaled effects belong to the commands that cause its file writes (for example `project.create_project@1` in `CORE.PRJ.CONT`).
- **Data structures:** `ProjectLocation { root, uuid, layout_version }`; `OpenMode { Read, Write }`; `Locked` guard (releases on drop); `LockInfo { pid, host, started }`; `StoreError { locked, network_fs_unsupported, layout_too_new, path_too_long, io }`.
- **Algorithm:** Windows paths are passed with the `\\?\` prefix internally; Linux and macOS use the paths as given. OS lock: `LockFileEx` on Windows, `flock` on Linux and macOS [unverified that `flock` is safe with SQLite's own locking; the lock file is separate from `project.db`]. Network file-system detection: volume type query on Windows, `statfs` file-system type on Linux and macOS [unverified completeness]. Projects use a folder rather than a single-file archive because SQLite WAL needs companion files and folders work with every sync tool's file-level model; sharing is an export.
- **`format` file:** line 1 `fid-project-layout@<n>`, line 2 `uuid=<lowercase hyphenated uuid>`. The UUID names the cache folder and is validated strictly because it becomes a path component.
- **Links:** symbolic links (on Windows also junctions and other reparse points) are followed only on the user's path to the project root; below the root the store refuses them (`symlink_refused`). Unix uses `O_NOFOLLOW` on every open below the root. Windows opens every entry with `FILE_FLAG_OPEN_REPARSE_POINT` and checks the reparse attribute on the opened handle, and keeps the directory handles it descends through open without delete sharing. While a project is open, those held handles also stop the project folder itself from being renamed or moved.
- **Lock identity:** on Linux and macOS a `flock` counts only if the locked file is still the file at `lock` (same device and inode), checked when the lock is taken and again by `Locked::verify()` before every write; a mismatch fails with `lock_lost`. On Windows the lock file is held open without write or delete sharing, so it cannot be renamed over, replaced or deleted while held.
- **Special files:** files below the root are opened non-blocking and must be regular files (`not_regular_file` otherwise), so a FIFO or device planted in a project cannot hang or redirect the store.
- **Manual recovery:** a record from another host, or a garbled record, is never taken over automatically. `ProjectLocation::force_unlock(path, confirmed_holder)` clears it only when no process holds the OS lock and the caller passes back the exact record shown in the `locked` error; it is meant for a human-confirmed command, never automatic.
- **Errors:** as in Data structures, plus `not_a_project`, `not_empty`, `invalid_name`, `lock_file_invalid`, `symlink_refused`, `lock_mismatch`, `not_regular_file` and `lock_lost` (added at build, PR #57).
- **Determinism:** D0 for path and name handling.
- **Performance budget:** open with lock in < 50 ms [unverified until measured].
- **Crate:** `fid-store` (module `layout`); `storage-writers.toml` at the workspace root listing `fid-store`.
- **Folder targets (REQ-011, added 2026-11-21, revised after review of #402):**
  - `ProjectLocation::create_at(&dyn VerifiedFolder)` and `ProjectLocation::open_at(&dyn VerifiedFolder, mode)` take a folder verified by `CORE.BUS.POL.FS` in place of a `&Path`. The trait gives the re-derived absolute path, the recorded identity and the read-only mark. This module defines it, and `fid-bus` implements it for `CreatedFolder` and `ProjectFolder`.
  - The opened directory's identity (`(st_dev, st_ino)`, or on Windows volume serial and file index) is compared with the recorded one twice: when the folder is opened, and again after `project.db` is opened. A mismatch either time is `outside_root`, and the store closes what it opened.
  - A read-only target allows `OpenMode::Read` only, and `open_at` with `Write` on it is `invariant_violated`. The database is opened with `SQLITE_OPEN_READONLY`. If it cannot be read without creating or writing a `-wal` or `-shm` file, the open fails with `read_only_unavailable`, which the command maps to `precondition/false`. Nothing in the folder is written.
  - The `&Path` forms stay for human raw paths and tests.
- **Atomic copy (REQ-012, REQ-013, added 2026-11-21, maintainer decision, revised after review of #402).** `ProjectLocation::copy_to(&self, target: &dyn CopyTarget, finish: impl FnOnce(&ProjectLocation, &Locked) -> Result<(), StoreError>, publish: impl FnOnce(&dyn Fn() -> Result<(), StoreError>) -> Result<(), StoreError>) -> Result<ProjectLocation, StoreError>`. `CopyTarget` is implemented by `fid-bus` from `FolderTarget::into_copy_target`, never from `create_new`, and gives the verified parent folder, its identity and the absent target name. The steps:
  1. **Temporary folder.** Create `.fid-copy-<32 lowercase hex random>` in the target's parent folder, then create and lock its `lock` at once (as `create` does). Liveness of that lock is how crash cleanup tells a live copy from a dead one. The name is fixed ASCII plus hex (REQ-007), and `CORE.BUS.POL.FS` refuses it as a handle. No `format` file is written yet, so a crash leaves a folder that is not a project.
  2. **Database.** Open the source with a read transaction and copy it with `sqlite3_backup_step(-1)`: the whole database in one step, so the copy is one consistent snapshot even while the source commits in WAL mode. This needs the `backup` feature of `rusqlite` (0.41.1), which `fid-store` must enable next to `bundled`. `project.db` and its `-wal`/`-shm` files are never copied as raw bytes. Then run `PRAGMA integrity_check` and the `CORE.STO.SQL.SCHEMA` tamper checks on the copy. Its head `H` is the source head the copy was taken at.
  3. **Objects (`CORE.STO.CAS`), transitively.** Every digest the snapshot references through `entry_blobs` and `snapshots` is copied by reading it through the CAS's verified read. For a chunked object, its `fid-chunks@1` manifest is copied and then every chunk it lists, each re-hashed against its own digest. A mismatch anywhere is `blob_corrupt` and aborts the copy. A blob written after the snapshot is not copied, and none the snapshot references is missing. `kernels/` is copied the same way. `tmp/` and the source `lock` are never copied.
  4. **Finish.** Write `format` with a new store-instance UUID (the copy has its own cache) and run `finish` under the temporary folder's lock (for example `CORE.PRJ.CONT.LIFE`'s session header and genesis). An error from `finish` aborts the copy.
  5. **Close, then fsync.** fsync every file, every folder and the temporary folder, then close the copy's database connection, release its lock and drop every handle into it. Windows cannot rename a folder with open handles inside it, and the same order is used everywhere. Folder fsync on Windows tolerates the same refusals as `CORE.STO.CAS` (`cas.rs` `sync_dir`: `FlushFileBuffers` on a directory handle, where access denied or invalid function is accepted because NTFS journals the rename [unverified for every file system]).
  6. **Publish.** Call `publish(rename)`. The caller runs its own commit-stage check and calls `rename` at most once. `rename` performs the no-replace rename to the target name:
     - Linux and macOS: `rustix::fs::renameat_with` with `RenameFlags::NOREPLACE` (`RENAME_NOREPLACE`, `RENAME_EXCL` on macOS). `rustix` 1 is already a `fid-store` dependency. A file system without support (`EINVAL`) fails closed with `io`, never with a replacing rename.
     - Windows: `winx` 0.36 (already in the lock through `cap-std`, and a `fid-bus` dependency) gives the file identities (`winapi_util::file::information`), but it has no move wrapper, and FFI would need `unsafe`, which the workspace forbids. So the build uses `std::fs::rename` after re-checking that the name is absent, and checks the target's identity after the rename. A non-empty folder cannot be replaced by a rename, so the move fails. The remaining race (an empty folder created at the name between the check and the rename) is documented [unverified whether `std`'s `FileRenameInfoEx` path replaces an empty folder; the build adds a test and, if it does, must find a safe no-replace call before merging].
     - Then fsync the parent folder and reopen the copy with `open_at` and the identity check (the identity survives a same-volume rename).
  7. **Cleanup.** If `publish` returns an error without calling `rename`, or any step fails, remove the temporary folder (best effort, logged) and return the error. After a crash, `copy_to` deletes any `.fid-copy-*` sibling whose `lock` is not held by a live process. Only folders matching that exact pattern are ever deleted.
  - **Errors.** An existing target (at the check or at the rename) is `not_empty`. Commands map it to `policy/would_overwrite` (`CORE.BUS.POL.FS` REQ-005). The new codes are `blob_corrupt` and `read_only_unavailable`.
- **Project UUID vs project ID:** the `format` UUID names the store instance and its cache folder. A copy gets a new one. It is not the project ID. The project ID and namespace are the identity record of `CORE.PRJ.CONT.LIFE` (the verified genesis entry), so neither value has two sources of truth.

## Test fixtures

`fixtures/core/sto/layout/` (stale lock files, a session generator); long-path tests generate their paths at run time. CC-BY-4.0.

## Build prompt

> In crate `fid-store`, add module `layout`: `ProjectLocation::create/open(path, mode) -> Result<(ProjectLocation, Option<Locked>), StoreError>`. A project is a folder with `format` (`fid-project-layout@1`, then `uuid=<project uuid>`), `project.db` (+ WAL files), `blobs/`, `kernels/`, `tmp/`, `lock`; the cache goes to `<user cache dir>/fiducial/cache/<project uuid>/`. Writing needs an exclusive OS lock on `lock` (LockFileEx on Windows, flock elsewhere) with PID, host and start time written in the file; a second writer gets `locked` with PID and host; a dead same-host holder is taken over with a `stale_lock_taken` warning; other-host holders are never taken over. Support 1,024-character paths (Windows `\\?\`). Only fixed lowercase ASCII names or hex digests inside a project. Refuse write opens on network file systems (`network_fs_unsupported`) and newer layouts (`layout_too_new`). Publish `storage-writers.toml` for `CORE.BUS.LINT`. Tests AC-1..AC-7 on Windows, Linux and macOS CI. Constraints: CONSTITUTION C7, §8 (no panic, no `unsafe`; all paths hostile).

## Open questions

- **Resolved (folder vs archive):** folder (see Design). Maintainers may still want a single-file project as a product choice; if so, a decision should be opened then.
- **Resolved (network shares):** write opens refused, read-only allowed; whether read-only WAL databases open reliably on SMB is [unverified].
- **Resolved at build (name of the marker file):** REQ-001 named it `FORMAT`, which conflicts with REQ-007 and AC-5 (lowercase names only); it is `format`.
- **Project UUID source of truth:** the UUID lives in line 2 of `format`. A later project table (`CORE.PRJ`, `CORE.STO.SQL`) must read it from there or keep it identical, so that the UUID never has two sources of truth. **Resolved 2026-11-21:** it is the store-instance UUID only. The project ID lives in the genesis entry (`CORE.PRJ.CONT.LIFE`), see Design.
- **Host identity:** the lock record's host is the `uname` node name (Linux, macOS) or `COMPUTERNAME` (Windows). Renaming a machine makes its own stale locks look foreign; recovery is the human-confirmed `force_unlock` above. A more stable machine identity (for example a machine UUID) may replace the host name later [unverified that one is readable on all three OS without platform FFI].
- **Residual link race on Windows:** directory listings (`list`) still go by path, and the project root's own parent chain is not held, so the root itself can be renamed away while it is open. Writes and reads below the root are handle-checked, so they cannot be redirected through a link. A fully handle-relative design would need `NtCreateFile` with a root directory handle, which means FFI [unverified that it is needed].
- `kernels/` is reserved for kernel artifacts carried with a project (`CORE.RPL.EXEC.VERIFY`, D-020).
- **Pending build:** REQ-011 to REQ-013 and AC-8 to AC-10 (added 2026-11-21) are not yet implemented. `built_in` covers REQ-001 to REQ-010 and AC-1 to AC-7; the REQ-001/002 wording change (store-instance UUID) matches what is built. Code doc comments are written in the build PR.

## Change log

- 2026-11-21: REQ-011 to REQ-013 and AC-8 to AC-10 added. Projects are opened from `CORE.BUS.POL.FS` folder targets with an identity check. The atomic project copy uses a temporary sibling folder, the SQLite online backup API, re-hashed blobs, fsync and a no-replace rename, and no partial project survives a crash. The `format` UUID is clarified as the store-instance UUID, not the project ID. Maintainer decision, to unblock `CORE.PRJ.CONT.LIFE`. No existing requirement changes.

- 2026-11-21 (review of #402): REQ-001 and REQ-002 now say "store-instance UUID" (wording only; what is built is unchanged). REQ-011 re-checks the folder identity after `project.db` is opened and defines read-only targets. REQ-012 takes the target from `into_copy_target`, locks the temporary folder at creation, copies chunked objects transitively and creates the target only by its no-replace rename, after closing every handle. Copy publication goes through a caller `publish` step, so the caller's commit check and the rename are ordered. The dependencies are named (`rusqlite` `backup` feature, `rustix`, `winx`), and the Windows rename limit is stated. Nothing is renumbered.

## Children

_Leaf: none._
