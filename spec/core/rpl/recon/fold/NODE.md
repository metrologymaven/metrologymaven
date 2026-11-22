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
built_in: [fid-replay]
built_pr: 458
updated: 2026-11-22
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
- **REQ-CORE.RPL.RECON.FOLD-008** (authority-bearing folds) Where state rebuilt from journal events carries authority, the fold that rebuilds it shall not trust event bodies for their origin. The rules are under "Authority-bearing folds (rule of record)" in Design. Such state includes approvals, branches and their protection, promotions, governance profiles, sign-offs and any other state that later decides whether a command may run. Each fold shall:
  - **(a) Origin.** Bind every event to its entry's verified origin, taken from the verified header and never from the event body. The origin is the principal (kind and committed identity), the command `id@major`, the parameters commitment, and the branch when branches matter.
  - **(b) Reserved effects.** Reserve the event's effect names in `RESERVED_EFFECTS` (`fid-registry`), so that only the owning command emits them, and refuse the event under any other command.
  - **(c) Requests.** Refuse an event whose entry also carries a `Request*` event, unless the owning command explicitly allows approved requests and the fold checks the approval.
  - **(d) Decided-against state.** Bind every event to the state it was decided against: a head, a sequence or a prior digest. The commit stage shall check that binding atomically with the commit, as a compare-and-swap, so that a replayed event (state A to B and back to A) and a concurrent commit against the same prior state are both refused.
  - **(e) Branches.** State which branches count for the fold, and how promoted copies (entries carrying `promoted_from`) are treated.
  - **(f) Refusals.** State whether a committed event the fold refuses is fatal (the fold fails closed and names the entry) or skipped (ignored, with the reason recorded). Fatal is the default. Skipping is allowed only where the leaf says why an ignored event cannot carry authority.
  - **(g) Forged events.** Ship oracle cases for forged events that cover each of (a) to (e), generated by the leaf's independent reference script.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/rpl/recon/fold/session_1200` with snapshots, when reconstructed at its head, then the state digest equals `session_1200.state_digest`; with the cache deleted, the digest is the same.
- **AC-2** Given a crash harness that kills the process at 200 random instants (seed 7) while committing to `session_1200`, when each run is reopened, then the reconstructed state equals the fold of exactly the commits that returned success before the kill.
- **AC-3** Given undo of 3 entries then redo of 2, when reconstructed after each move, then each digest equals the digest recorded for that head in the fixture.
- **AC-4** (error path) Given an entry with a detached payload and no covering snapshot, then reconstruction fails with `payload_missing` and the live state is unchanged.
- **AC-5** Given a toy `DocState` with an injected bug that drops one event kind, when the fold-invariant hook runs, then it reports a mismatch and one integrity incident is logged.
- **AC-6** Given fixture `fixtures/core/rpl/recon/fold/bench_10000`, when reconstructed on reference hardware, then it completes in under 2 s (benchmark gate) [unverified until measured].
- **AC-7** (error path, REQ-008) Given any leaf that folds authority-bearing state, when its forged-event oracle cases run, then the fold refuses (or, where the leaf says so, skips) each of these: an event from another principal, command, major or parameters commitment, or from the wrong branch (a); a reserved effect emitted by another command (b); an event beside an unapproved `Request*` event (c); a replayed event and the second of two concurrent events decided against the same state (d); and a promoted copy or side-branch event handled differently from the leaf's stated rule (e). Each leaf's own tests carry these cases. This node's review checks that they exist.

## Design

- **Registry command(s):** none; library called by `CORE.PRJ.CONT` on open and by undo, redo and branch switch through an injected `Reconstructor` trait.
- **Data structures:** `Reconstructed<S: DocState> { state: S, head: EntryHash, digest: Digest, from_snapshot: Option<EntryHash> }`; `FoldError { payload_missing, apply_failed, decode_failed }`.
- **Algorithm:** `SNAP::select_valid(head)` → path from snapshot entry to head via logical parents (ordered) → for each entry read the payload event bodies, verify them against header commitments, `apply` in order → digest. Crash resume needs no special code: durable-only reads give it.
- **Errors:** as in Data structures.
- **Determinism:** D0; event order fixed by the logical path; no clock or randomness (close-time sampling uses a counter).
- **Performance budget:** REQ-007.
- **Crate:** `fid-replay` (module `fold`).

### Authority-bearing folds (rule of record, REQ-008)

The generic fold of this node trusts event bodies, because it only rebuilds document state that was already decided. A fold whose result decides later commands is different: a forged or replayed event in it grants authority. REQ-008 applies to every such fold, whichever crate holds it: the bus policy projection, journal branches and promotion, the governance profile, and future sign-off or approval state. The pattern the existing builds use, and that later builds shall follow:

- **Origin record.** The fold takes, per entry, a record built only from the verified header and the commit stage. That record holds the bus `EntryOrigin` (principal, command `id@major`, parameters commitment), the journal `EntryOrigin` (kind, command, class, branch, adding stage), `promoted_from`, and the entry hash. Event bodies are checked against it, and never the other way round.
- **Precommit equals fold.** The handler checks its event against the same rules under the origin its entry will record. The commit stage runs that check atomically with the commit (compare-and-swap on the decided-against head, sequence or digest), and a mismatch is `conflict/head_moved`. So a committed event the fold refuses can only come from a commit-stage bug or a forged journal, which is why fatal is the default under (f).
- **One owner per effect.** The effect names are listed in `fid_registry::RESERVED_EFFECTS` against their owning command, and the registry build refuses another command that declares them.
- **Requests stay requests.** `Request*` events record that an agent asked. They never carry the authority themselves. An entry holding both a `Request*` event and an authority event is refused, unless the command's spec allows approved requests and the fold checks the approval.
- **Oracle.** The leaf's stdlib reference script generates the forged cases, one at least per item (a) to (e), as the #378 build did (30 cases).

Rationale: four builds re-learned this in review:
- **#328** (`CORE.BUS.POL.PROJ`, approvals) introduced `EntryOrigin`, after approvals were folded from bodies alone.
- **#356** (`CORE.JRN.BR.NAME`, branches) bound the branch fold to each entry's origin and adding stage.
- **#366** (`CORE.JRN.BR.PROMO`, promotions) bound promotions to their source and target heads, the diff and the verified human assertion.
- **#378** (`TRUST.PROFILE.SELECT`, governance profile) added the decided-against binding with commit-stage compare-and-swap, after an A→B→A replay and a concurrent pair were found.

## Test fixtures

`fixtures/core/rpl/recon/fold/` synthetic sessions and a 10,000-entry benchmark generated with a fixed seed; toy `DocState`; crash harness shared with `CORE.STO.SQL.CRASH`. CC-BY-4.0.

## Build prompt

> In crate `fid-replay`, add module `fold` with `reconstruct<S: DocState>(journal, head) -> Result<Reconstructed<S>, FoldError>`: take the starting snapshot from `snapshot::select_valid`, walk the logical path to the head, verify each payload's event bodies against the header commitments, apply them in order (trusting them, no re-execution), and return state plus digest. Read only durable entries. Use it for open, undo, redo and branch switch. Add `check_fold_invariant(live, journal, head)` and a close-time sampler that runs at least 1 close in 20 by counter and logs an integrity incident on mismatch. Detached payload without covering snapshot fails with `payload_missing` and leaves the live state untouched. Tests AC-1..AC-6, including the crash harness (200 kills, seed 7) and the 10,000-entry benchmark. Constraints: CONSTITUTION §4, §6, §8 (no panic, no `unsafe`), REQ "no committed command lost".

## Open questions

- GPU device loss does not touch CPU state; this leaf only guarantees the state the viewer rebuilds from. `VIZ` owns the resource re-creation test.

## Change log

- 2026-11-20: REQ-008 and AC-7 added, with the Design section "Authority-bearing folds (rule of record)". They write down the rule for folds of authority-bearing journal state that the reviews of #328, #356, #366 and #378 each re-derived: bind to the verified origin, reserve effects, refuse unapproved requests, bind to the decided-against state with a commit-stage compare-and-swap, state the branch rule and the refusal policy, and ship forged-event oracle cases. No existing requirement changes.

## Children

_Leaf: none._
