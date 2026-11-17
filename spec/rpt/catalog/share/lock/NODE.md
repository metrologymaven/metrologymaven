---
id: RPT.CATALOG.SHARE.LOCK
title: Single-writer locking and file-share safety check
level: 4
parent: RPT.CATALOG.SHARE
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.CATALOG.SHARE-003, REQ-RPT.CATALOG.SHARE-004]
covers: []
depends_on: [RPT.CATALOG.STORE.SCHEMA, CORE.STO.LAYOUT]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# RPT.CATALOG.SHARE.LOCK: Single-writer locking and file-share safety check

## Purpose

A shared catalog is one SQLite file on a file share, written by its owner and read by many. SQLite's own locking is known to be unreliable on some network file systems [unverified for SMB and NFS specifics], so this task leaf adds an explicit single-writer lock file with owner identity and lease, opens readers on an immutable snapshot copy when safe concurrent reading cannot be shown, and runs a probe that decides whether a share is safe to use at all. No server is involved: only the file share.

## Scope

**In:** writer lock file (`catalog.lock`: owner ID, host, process, lease expiry), lease renewal and stale-lock takeover by the owner only; publication of a read snapshot (`catalog.snapshot.sqlite` plus digest) after each writer session; the share safety probe (exclusive create, rename atomicity, byte-range lock test); results `safe`, `snapshot_only`, `unsafe`.

**Out:** attach, detach and ownership rules (`RPT.CATALOG.SHARE.ATTACH`); queries (`RPT.CATALOG.SHARE.FED`); hosted servers (`ENT.SRV`).

## Requirements

- **REQ-RPT.CATALOG.SHARE.LOCK-001** A writer shall hold the lock file, created with exclusive create, before any write to a shared catalog, and if another live lease exists, then the write shall fail with error `stats.share_locked` naming the holder.
- **REQ-RPT.CATALOG.SHARE.LOCK-002** At the end of each writer session the writer shall publish a read snapshot by writing a new file and renaming it into place, together with its SHA-256 digest.
- **REQ-RPT.CATALOG.SHARE.LOCK-003** The safety probe shall classify a share as `safe`, `snapshot_only` or `unsafe`, and if exclusive create or atomic rename fails, then it shall return `unsafe`.
- **REQ-RPT.CATALOG.SHARE.LOCK-004** Readers on a `snapshot_only` share shall open only the published snapshot read-only and verify its digest before use.
- **REQ-RPT.CATALOG.SHARE.LOCK-005** The lock and snapshot mechanism shall use no network service other than the file share and no account (CONSTITUTION C7).

## Acceptance criteria

- **AC-1** Given a local temporary folder standing in for a share, when two writer processes start, then one holds the lock and the other fails with `stats.share_locked` naming the first.
- **AC-2** Given a writer session that adds 100 entries, then a new snapshot and digest are published and a reader opening it sees 100 more entries.
- **AC-3** Given a test file system shim that fails exclusive create, then the probe returns `unsafe`.
- **AC-4** Given a snapshot whose bytes were altered, then a reader refuses it with `stats.snapshot_digest_mismatch`.
- **AC-5** (manual, documented) The probe is run on an SMB and an NFS share in the release test matrix and its classification is published [unverified until run].

## Design

- **Registry commands:** none (library used by ATTACH and the writer commands).
- **API:** `WriterLock::acquire(dir, owner) -> Result<WriterLock, LockError>`; `publish_snapshot(catalog, dir)`; `probe(dir) -> ShareSafety`.
- **Errors:** `stats.share_locked { holder }`, `stats.share_unsafe`, `stats.snapshot_digest_mismatch`.
- **Determinism:** N (file-system observation); snapshot digests are D0.
- **Performance budget:** probe under 2 s on a LAN share [unverified until measured].
- **Crate:** `fid-catalog` (module `share::lock`).

## Test fixtures

- Temporary folders and a fault-injecting file-system shim; no external data.

## Build prompt

> In crate `fid-catalog`, module `share::lock`, implement single-writer locking for a catalog on a file share: an exclusive-create lock file with owner ID, host, process and lease (`stats.share_locked { holder }` if a live lease exists; only the owner may take over a stale lease), snapshot publication by write-then-rename with SHA-256 digest at the end of each writer session, digest-checked read-only snapshot opening, and a probe that classifies a folder as `safe`, `snapshot_only` or `unsafe` (exclusive create, rename atomicity, byte-range lock). No network service other than the share. Tests AC-1 to AC-4 with temporary folders and a fault-injecting shim. CONSTITUTION §8 applies. Spec: `spec/rpt/catalog/share/lock/NODE.md`.

## Open questions

- SQLite over SMB and NFS has known locking hazards [unverified specifics]; AC-5 must be run on real shares before release.

## Children

_Leaf: none._
