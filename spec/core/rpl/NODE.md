---
id: CORE.RPL
title: Replay engine and state reconstruction
level: 2
parent: CORE
kind: subsystem
status: split
phase: P0
size: XL
owners: []
claimed_by:
satisfies: [REQ-CORE-005, REQ-CORE-008]
covers: []
depends_on: [CORE.JRN, CORE.BUS, CORE.REG, CORE.STO]
cli: [fid journal replay*]
mcp: [journal_replay*]
journal: [journal.replay*]
updated: 2026-11-16
---

# CORE.RPL: Replay engine and state reconstruction

## Purpose

Brings state back from the journal and checks it. RPL has two jobs:
- **reconstruct**: rebuild the live document from the latest verified snapshot plus recorded events, used when a project opens or a session resumes after a crash;
- **re-execute**: run recorded commands again through the bus and compare digests, in the five replay modes of CONSTITUTION §4, reporting one of `IDENTICAL | EQUIVALENT | DIVERGENT | NOT_REPLAYABLE`.

It is separate from the journal because it reads entries and never appends to the source journal (replay output goes to a new branch or a report).

## Scope

**In:**
- Reconstruct from snapshot plus events; snapshot creation and validation (snapshots are caches derived from a journal prefix)
- Session resume after crash or GPU device loss
- Exact re-execution with observation substitution for class-N inputs
- Re-targeted replay: a journal span or program re-run with new input bindings
- Verifier replay: re-execution under a recorded or newer kernel in a sandbox, producing a Replay Report
- Dry-run replay on a throwaway branch, producing a changeset diff
- Cross-version diff: the same inputs under two kernel versions, reporting per-characteristic deltas and decision changes
- Outcome classification by determinism class, with decision invariance (any decision change is `DIVERGENT`) and the informative `MARGINAL` annotation
- First-divergence search by bisection and attribution (kernel, input, parameter, defaults profile, command migration)
- Interactive step mode API (the viewer synchronisation is VIZ)
- Fold invariant check: digest of live state equals digest of folded journal

**Out** (and which node owns it instead):
- The equivalence tolerance table per quantity kind (owned by `UNC` / `KRN` verification data; RPL consumes it)
- Command Programs, their language and record-to-program editing (owned by `AUTO`)
- Signing Replay Reports and the standalone verifier binary (owned by `TRUST`)
- Motion simulation for dry-run of M1 requests (owned by `CMM`)
- Device playback from transcripts (owned by `HAL`)
- Step-mode UI (owned by `VIZ`)

## Requirements

- **REQ-CORE.RPL-001** When a project is opened, the replay engine shall reconstruct state from the newest snapshot whose digest verifies plus all later recorded events.
- **REQ-CORE.RPL-002** If a snapshot's digest does not match its recorded digest, then the replay engine shall discard it and reconstruct from an earlier snapshot or from the journal start.
- **REQ-CORE.RPL-003** When the process restarts after a crash or a GPU device loss, the replay engine shall restore the state as of the last durable journal entry with no committed command missing.
- **REQ-CORE.RPL-004** When replaying in exact mode, the replay engine shall substitute every class-N input (instrument data, wall clock, human text, LLM output, external files) from the recorded digest and never regenerate it.
- **REQ-CORE.RPL-005** If a recorded input blob, command major or kernel version needed for replay is missing, then the replay engine shall report `NOT_REPLAYABLE` and name the missing item.
- **REQ-CORE.RPL-006** When every output digest of a replay equals the recorded digest, the replay engine shall report `IDENTICAL`.
- **REQ-CORE.RPL-007** When outputs differ only within the equivalence tolerance for their quantity kind and no decision or status changes, the replay engine shall report `EQUIVALENT`.
- **REQ-CORE.RPL-008** If any conformance decision or characteristic status differs from the recording, then the replay engine shall report `DIVERGENT` regardless of numeric tolerance.
- **REQ-CORE.RPL-009** If a D0, D1 or D2 step replayed with the same kernel manifest digest on the same platform class is not bit-identical, then the replay engine shall report `DIVERGENT` for that step.
- **REQ-CORE.RPL-010** When replaying in re-targeted mode, the replay engine shall bind the named inputs to new data and write results to a new branch, never to the source branch.
- **REQ-CORE.RPL-011** When replaying in dry-run mode, the replay engine shall execute on a throwaway branch and return a changeset diff without changing any persistent branch.
- **REQ-CORE.RPL-012** When replaying in cross-version-diff mode, the replay engine shall report, per characteristic, the value delta and any decision change between the two kernel versions.
- **REQ-CORE.RPL-013** When a replay is `DIVERGENT`, the replay report shall identify the first divergent entry and attribute it to one of: kernel, input, parameter, defaults profile or command migration.
- **REQ-CORE.RPL-014** Verifier replay shall never use the memoisation cache; it shall recompute every step.
- **REQ-CORE.RPL-015** Reconstructing a project of 10,000 journal entries from a snapshot no more than 500 entries old shall complete in under 2 s on reference hardware [unverified until measured].

## Interfaces

- **Offers:** `Replay::reconstruct(project) -> State`, `Replay::run(mode, span, bindings) -> ReplayReport`, `Replay::step(cursor)`; the Replay Report JSON schema (consumed by TRUST and RPT).
- **Commands:** `journal.replay_exact@1` (R1), `journal.replay_retargeted@1` (W2 to a new branch), `journal.replay_dry_run@1` (R1), `journal.replay_cross_version@1` (R1), `journal.replay_verify@1` (R1). CLI `fid journal replay-*`; MCP `journal_replay_*`. Names fixed at leaf level.
- **Consumes:** `CORE.JRN` entries and heads, `CORE.BUS` for re-execution, `CORE.REG` for old majors and migrators, `CORE.STO` for snapshots and blobs; equivalence tolerances from `UNC`/`KRN`.

## Decomposition hints

RECON (snapshots, reconstruct, resume), EXACT (exact re-execution and observation substitution), CLASS (outcome classification and decision invariance), RETGT (re-targeted and dry-run), XVER (cross-version diff), BISECT (first divergence and attribution), STEP (step-mode API).

## Open questions

- The normative equivalence tolerance table (REP-1 in the concept docs) has no owner node yet. RPL assumes UNC or KRN publishes it; if neither does, a decision is needed.
- Verifier replay overlaps with TRUST's `fid-verify`. Proposed boundary: RPL is the engine, TRUST packages it into the free verifier binary.

## Split rationale

The dividing principle is **reconstruct versus re-execute, then by mode, with the shared judges pulled out**: RECON brings state back by trusting events (open, resume, head moves); EXEC re-executes to check (exact and verifier modes, observation substitution, Replay Report); RETGT re-executes to produce new state (re-targeted, dry-run, and rebase for promotion); XVER re-executes under two kernels. CLASS (outcome classification) and BISECT (first divergence and attribution) are pure functions every mode uses, so they are separate leaves.

The equivalence tolerance table (REP-1) is owned by `UNC.RDL` per the level-2 review; CLASS consumes it through a trait, which resolves the parent's first open question. Step mode is an API on EXEC's cursor rather than its own child, because it has no requirement of its own.

Alternatives considered:
- **One child per replay mode, including classification inside each.** Rejected: it would duplicate the decision-invariance rule five times.
- **Dry-run in EXEC.** Rejected: dry-run creates (throwaway) state and needs the same branch handling as re-targeted replay, not the checking path.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| CORE.RPL.RECON | Snapshots, reconstruction and crash resume | 001 002 003 015 | none |
| CORE.RPL.EXEC | Exact and verifier re-execution with observation substitution | 004 005 014 | none |
| CORE.RPL.CLASS | Replay outcome classification and decision invariance (leaf) | 006 007 008 009 | none |
| CORE.RPL.RETGT | Re-targeted, dry-run and rebase re-execution | 010 011 | none |
| CORE.RPL.XVER | Cross-version diff | 012 | none |
| CORE.RPL.BISECT | First-divergence search and attribution (leaf) | 013 | none |
