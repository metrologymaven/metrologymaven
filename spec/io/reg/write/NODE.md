---
id: IO.REG.WRITE
title: Atomic export writes and overwrite protection
level: 3
parent: IO.REG
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-io-reg]
satisfies: [REQ-IO.REG-016, REQ-IO.REG-017]
covers: []
depends_on: [IO.REG.ROOTS, IO.REG.LIMITS]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# IO.REG.WRITE: Atomic export writes and overwrite protection

## Purpose

The single code path through which every writer puts bytes on disk inside an export root: write to a temporary file in the same directory, flush, then rename into place, so a target is either the complete new file or untouched. It also enforces `overwrite`. Writing into a local export root is W1 with `fs_effect: write_root` (CONSTITUTION §3.1).

## Scope

**In:**
- `AtomicWriter`: temp file creation next to the target, streaming write, fsync, rename, cleanup on error, cancel or drop
- Overwrite rule and `target_exists`
- Output digest (SHA-256) and byte count returned for the export journal event

**Out:**
- Root-handle resolution (`IO.REG.ROOTS`)
- Format serialisation (format children)
- Egress to remote destinations (X1; not offered by IO)

## Requirements

- **REQ-IO.REG.WRITE-001** If the export target exists and `overwrite` is not set, then the export shall fail with error `target_exists` before any byte is written.
- **REQ-IO.REG.WRITE-002** The writer shall write to a temporary file in the target's directory and rename it onto the target only after all bytes are written and flushed.
- **REQ-IO.REG.WRITE-003** If the write fails, is cancelled or the writer is dropped before commit, then the temporary file shall be removed and the target shall be unchanged.
- **REQ-IO.REG.WRITE-004** When a write commits, the writer shall return the SHA-256 digest and byte count of the committed file.

## Acceptance criteria

- **AC-1** Given an export root with no `out.stl`, when 1 MiB is written and committed, then `out.stl` exists with the returned digest and size, and no temp file remains.
- **AC-2** (error path) Given an existing `out.stl` and `overwrite=false`, then the call fails `target_exists` and the file's digest is unchanged.
- **AC-3** Given an existing `out.stl` and `overwrite=true`, when a write fails after 512 KiB (injected I/O error), then `out.stl` still has its old digest and no temp file remains.
- **AC-4** Given a writer dropped without commit, then no temp file remains.
- **AC-5** Given a crash simulated by killing a child process mid-write, when the directory is listed afterwards, then the target is either absent or the old file; stale temp files match the pattern `.fid-tmp-*` and are removed by the next `AtomicWriter` in that directory if older than 1 hour.

## Design

- **Registry commands:** none (library used by `IO.REG.ENV` export commands, class W1 `fs_effect: write_root`).
- **Data structures:** `AtomicWriter { dir, target_name, tmp, hasher, bytes }` with `write`, `commit() -> Result<WriteReceipt, IoError>`; `WriteReceipt { sha256, bytes }`.
- **Algorithm:** temp name `.fid-tmp-<uuidv7>`; create exclusive; stream with hashing; `sync_all`; rename (on Windows, `MoveFileEx` with replace semantics through std `fs::rename`); best-effort directory fsync on POSIX.
- **Errors:** `target_exists`, `io{kind}`.
- **Determinism:** D0 for bytes and digest; temp names are not part of any record.
- **Performance budget:** throughput within 10% of a plain buffered write [unverified until measured].
- **Crate:** `fid-io-reg`.

## Test fixtures

Temp directories created per test; fault injection through a `Write` wrapper that fails after N bytes.

## Build prompt

> In crate `fid-io-reg`, implement `AtomicWriter`: given a resolved export-root directory handle and target name, fail `target_exists` if the target exists and `overwrite` is false; write to `.fid-tmp-<uuidv7>` in the same directory while hashing (SHA-256); on `commit` flush, sync and rename onto the target and return `{sha256, bytes}`; on error, cancel or Drop remove the temp file. Clean stale `.fid-tmp-*` files older than one hour. Side-effect class of exports using it: W1 with `fs_effect: write_root` (CONSTITUTION §3.1). No panics, no unwrap (CONSTITUTION §8). Tests AC-1..AC-5.

## Open questions

- Network-share export roots behave differently on rename; any non-local root is X1 and out of IO's scope until a decision says otherwise.

## Children

_Leaf: none._
