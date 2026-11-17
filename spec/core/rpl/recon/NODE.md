---
id: CORE.RPL.RECON
title: Snapshots, reconstruction and crash resume
level: 3
parent: CORE.RPL
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-CORE.RPL-001, REQ-CORE.RPL-002, REQ-CORE.RPL-003, REQ-CORE.RPL-015]
covers: []
depends_on: [CORE.JRN, CORE.STO]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# CORE.RPL.RECON: Snapshots, reconstruction and crash resume

## Purpose

Brings the live document back from storage. RECON takes snapshots (materialised states derived from a journal prefix), validates them, and rebuilds state from the newest valid snapshot plus the recorded events after it. It is used on every project open, after undo/redo head moves, and after a crash or GPU device loss. It also owns the fold invariant: the digest of live state must equal the digest of state folded from the journal.

## Scope

**In:**
- Snapshot creation policy (every N entries or on close), `SnapshotTaken{entry, digest}` records, snapshot validation
- Reconstruction: apply recorded events (trusting them) from a snapshot to a head
- Fallback to an older snapshot or the journal start when a snapshot fails its digest
- Resume after process crash or GPU device loss
- Fold invariant check (live digest equals folded digest) as a test hook and a sampled check on project close
- Reconstruction performance budget

**Out** (and which node owns it instead):
- Re-executing commands and comparing digests (owned by `CORE.RPL.EXEC`)
- Snapshot byte storage and cache eviction (owned by `CORE.STO`)
- The state model being reconstructed (owned by `CORE.PRJ`)

## Requirements

- **REQ-CORE.RPL.RECON-001** When a project is opened, the replay engine shall reconstruct state from the newest snapshot whose digest verifies plus all later recorded events on the current branch.
- **REQ-CORE.RPL.RECON-002** If a snapshot's content digest differs from its recorded digest, then the replay engine shall discard it, record an integrity warning in the Activity Log, and fall back to an earlier snapshot or the journal start.
- **REQ-CORE.RPL.RECON-003** When the process restarts after a crash or a GPU device loss, the replay engine shall restore the state as of the last durable journal entry with no committed command missing.
- **REQ-CORE.RPL.RECON-004** The replay engine shall take a snapshot at least every 500 entries and at every project close.
- **REQ-CORE.RPL.RECON-005** When a head moves (undo, redo, branch switch), the replay engine shall reconstruct the state at the new head from the nearest valid snapshot on that logical path.
- **REQ-CORE.RPL.RECON-006** The digest of the live state shall equal the digest of the state reconstructed from the journal at the same head; a mismatch found by the sampled check on project close shall be recorded as an integrity incident.
- **REQ-CORE.RPL.RECON-007** Reconstructing a project of 10,000 journal entries from a snapshot no more than 500 entries old shall complete in under 2 s on reference hardware [unverified until measured].

## Interfaces

- **Offers:** `Replay::reconstruct(project, head) -> State`, `Snapshot::take/validate`, fold-invariant test hook.
- **Consumes:** `CORE.JRN` (entries, heads, `SnapshotTaken` records), `CORE.STO` (snapshot blobs, cache).

## Decomposition hints

SNAP (snapshot policy, take and validate), FOLD (apply events, fold invariant), RESUME (crash and device-loss resume tests), PERF (reconstruction benchmark).

## Open questions

- Snapshot content is "QIF projection plus indices" in the concept docs; the exact projection depends on `CORE.PRJ.QIF`.

## Children

_None yet: status `open`._
