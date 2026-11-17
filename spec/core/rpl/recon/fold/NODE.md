---
id: CORE.RPL.RECON.FOLD
title: Reconstruction, head moves, fold invariant and crash resume
level: 4
parent: CORE.RPL.RECON
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.RPL.RECON-001, REQ-CORE.RPL.RECON-003, REQ-CORE.RPL.RECON-005, REQ-CORE.RPL.RECON-006, REQ-CORE.RPL.RECON-007]
covers: []
depends_on: [CORE.RPL.RECON.SNAP, CORE.JRN.CHAIN, CORE.JRN.ENTRY]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.RPL.RECON.FOLD: Reconstruction, head moves, fold invariant and crash resume

## Purpose

Rebuilds the document state at a head by starting from the snapshot SNAP selects and applying the recorded event bodies of every later entry on that logical path, trusting them (no re-execution). The same path serves project open, undo/redo and branch switches, and resume after a crash, because only durable entries are ever read. FOLD also checks the fold invariant: the live state's digest equals the digest of the state folded from the journal.

## Scope

**In:** `reconstruct(project, head)`, event application order, crash resume semantics, GPU device-loss resume (state is CPU-side; the viewer re-creates GPU resources from it), the fold-invariant test hook and sampled close-time check, the reconstruction benchmark.
**Out:** snapshot choice (`CORE.RPL.RECON.SNAP`); making entries durable (`CORE.JRN.CHAIN`, `CORE.STO.SQL.TXN`); re-creating GPU resources (`VIZ`); re-executing commands (`CORE.RPL.EXEC`).

## Requirements

- **REQ-CORE.RPL.RECON.FOLD-001** When asked to reconstruct a head, the engine shall decode the snapshot SNAP selects (or start from the empty state) and apply, in logical-path order, the event bodies of every Record Journal entry after the snapshot up to and including the head.
- **REQ-CORE.RPL.RECON.FOLD-002** The engine shall read only entries the journal reports as durable, so that after a crash the reconstructed state contains every commit that returned success and nothing else.
- **REQ-CORE.RPL.RECON.FOLD-003** When a head moves by undo, redo or branch switch, the engine shall reconstruct the new head through the same path and return the state and its digest.
- **REQ-CORE.RPL.RECON.FOLD-004** If an entry needed by the fold has a detached payload and no later valid snapshot covers it, then the engine shall fail with `payload_missing` naming the entry and leave the previous live state in place.
- **REQ-CORE.RPL.RECON.FOLD-005** The fold-invariant hook shall compare the live state's digest with the digest of a fresh reconstruction at the same head, and when the sampled check on project close finds a mismatch, the engine shall write an integrity incident naming the head to the Activity Log.
- **REQ-CORE.RPL.RECON.FOLD-006** The sampled close-time check shall run on at least 1 close in 20, chosen by a counter, not by randomness or time.
- **REQ-CORE.RPL.RECON.FOLD-007** Reconstructing a project of 10,000 journal entries from a snapshot no more than 500 entries old shall complete in under 2 s on reference hardware [unverified until measured].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/rpl/recon/fold/session_1200` with snapshots, when reconstructed at its head, then the state digest equals `session_1200.state_digest`; with the cache deleted, the digest is the same.
- **AC-2** Given a crash harness that kills the process at 200 random instants (seed 7) while committing to `session_1200`, when each run is reopened, then the reconstructed state equals the fold of exactly the commits that returned success before the kill.
- **AC-3** Given undo of 3 entries then redo of 2, when reconstructed after each move, then each digest equals the digest recorded for that head in the fixture.
- **AC-4** (error path) Given an entry with a detached payload and no covering snapshot, then reconstruction fails with `payload_missing` and the live state is unchanged.
- **AC-5** Given a toy `DocState` with an injected bug that drops one event kind, when the fold-invariant hook runs, then it reports a mismatch and one integrity incident is logged.
- **AC-6** Given fixture `fixtures/core/rpl/recon/fold/bench_10000`, when reconstructed on reference hardware, then it completes in under 2 s (benchmark gate) [unverified until measured].

## Design

- **Registry command(s):** none; library called by `CORE.PRJ.CONT` on open and by undo, redo and branch switch through an injected `Reconstructor` trait.
- **Data structures:** `Reconstructed<S: DocState> { state: S, head: EntryHash, digest: Digest, from_snapshot: Option<EntryHash> }`; `FoldError { payload_missing, apply_failed, decode_failed }`.
- **Algorithm:** `SNAP::select_valid(head)` → path from snapshot entry to head via logical parents (ordered) → for each entry read the payload event bodies, verify them against header commitments, `apply` in order → digest. Crash resume needs no special code: durable-only reads give it.
- **Errors:** as in Data structures.
- **Determinism:** D0; event order fixed by the logical path; no clock or randomness (close-time sampling uses a counter).
- **Performance budget:** REQ-007.
- **Crate:** `fid-replay` (module `fold`).

## Test fixtures

`fixtures/core/rpl/recon/fold/` synthetic sessions and a 10,000-entry benchmark generated with a fixed seed; toy `DocState`; crash harness shared with `CORE.STO.SQL.CRASH`. CC-BY-4.0.

## Build prompt

> In crate `fid-replay`, add module `fold` with `reconstruct<S: DocState>(journal, head) -> Result<Reconstructed<S>, FoldError>`: take the starting snapshot from `snapshot::select_valid`, walk the logical path to the head, verify each payload's event bodies against the header commitments, apply them in order (trusting them, no re-execution), and return state plus digest. Read only durable entries. Use it for open, undo, redo and branch switch. Add `check_fold_invariant(live, journal, head)` and a close-time sampler that runs at least 1 close in 20 by counter and logs an integrity incident on mismatch. Detached payload without covering snapshot fails with `payload_missing` and leaves the live state untouched. Tests AC-1..AC-6, including the crash harness (200 kills, seed 7) and the 10,000-entry benchmark. Constraints: CONSTITUTION §4, §6, §8 (no panic, no `unsafe`), REQ "no committed command lost".

## Open questions

- GPU device loss does not touch CPU state; this leaf only guarantees the state the viewer rebuilds from. `VIZ` owns the resource re-creation test.

## Children

_Leaf: none._
