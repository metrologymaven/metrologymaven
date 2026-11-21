---
id: AUTO.RUN.BIND
title: Run binding validation and run records
level: 3
parent: AUTO.RUN
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
built_in: [fid-run]
built_pr: 390
satisfies: [REQ-AUTO.RUN-001, REQ-AUTO.RUN-002]
covers: [PW.AUT.05]
depends_on: [AUTO.REC.LIFE, AUTO.PROG.FAMILY, CORE.PRJ]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.RUN.BIND: Run binding validation and run records

## Purpose

Everything that must be true before a run starts, and the record that says what ran. `prepare_run` checks the program's lifecycle state (released, or a human trial run), validates the parameter binding or variant against the program's declarations, digests every input, and produces the `RunRecord` that `AUTO.RUN.EXEC` writes as the project's Run entity together with the run's own journal branch.

## Scope

**In:**
- `prepare_run(program_ref, binding | variant, principal, flags) -> Result<RunPlan>`
- Lifecycle rules: released → allowed; draft or registered → only as a human trial run; deprecated → refused unless a human sets the override flag
- Binding validation via `AUTO.PROG.FAMILY`; input data digests
- `RunRecord` schema (the Run entity's fields) and its canonical form

**Out:**
- Executing the run and creating the branch (`AUTO.RUN.EXEC`)
- Lifecycle states (`AUTO.REC.LIFE`), Run entity storage (`CORE.PRJ`)

## Requirements

- **REQ-AUTO.RUN.BIND-001** `prepare_run` shall accept a released program for any principal allowed to run it, and shall refuse a draft or registered program with `not_released` unless a human principal requests a trial run.
- **REQ-AUTO.RUN.BIND-002** If the program is deprecated, then `prepare_run` shall refuse with `deprecated` unless a human principal sets the override flag, and the override shall be recorded in the run record.
- **REQ-AUTO.RUN.BIND-003** `prepare_run` shall validate the binding with `check_binding` and refuse with the full error list if any error is reported.
- **REQ-AUTO.RUN.BIND-004** The run record shall hold run ID (UUIDv7), program ID, version and digest, the binding as authored, the SHA-256 of every bound input dataset, the principal, the trial and override flags, the branch reference and the outcome.
- **REQ-AUTO.RUN.BIND-005** A trial run's record shall be marked `trial: true`, and results produced in it shall carry the trial mark.

## Acceptance criteria

- **AC-1** Given released fixture program `fixtures/program/family/bracket.json` and variant `B-120` with input scan `fixtures/scans/bracket-120.ply`, when `prepare_run` runs for an agent principal, then it returns a plan whose record holds the program digest, the variant binding and the scan's SHA-256.
- **AC-2** (error path) Given the same program in the registered state and an agent principal, then `not_released`; for a human principal with `trial`, then a plan with `trial: true`.
- **AC-3** (error path) Given a deprecated program, then `deprecated` for an agent; for a human with override, then a plan whose record shows the override.
- **AC-4** (error path) Given a binding missing `hole_spacing`, then refusal listing `missing_parameter`.
- **AC-5** Given the same inputs twice, then the two records are identical except run ID.

## Design

- **Registry command(s):** none; library used by `program.run@1` and `program.run_batch@1`.
- **Data structures:** `RunPlan { record: RunRecord, steps_retargetable: Vec<(StepId, bool)> }`; `RunRecord` as in REQ-AUTO.RUN.BIND-004, canonical JSON.
- **Algorithm:** lifecycle query → principal rules → binding check → stream SHA-256 of each input through root handles.
- **Errors:** `not_released`, `deprecated`, `binding_invalid`, `input_unreadable`, `program_not_found` (an identified program reference whose digest names no version).
- **Determinism:** D0 (apart from the run ID).
- **Performance budget:** dominated by hashing inputs; ≥ 500 MB/s on reference hardware [unverified until measured].
- **Crate:** `fid-run`

## Test fixtures

`fixtures/program/family/bracket.json` (shared), `fixtures/scans/bracket-120.ply` (synthetic, seeded generator, project-authored); lifecycle states set through the CORE test harness.

## Build prompt

> In a new crate `fid-run`, implement `prepare_run(program_ref, binding_or_variant, principal, flags)`. Rules: released programs run for any allowed principal; draft or registered only as a human trial run (`not_released` otherwise, record `trial: true`); deprecated only with a human override (`deprecated` otherwise, record the override). Validate the binding with `fid_program::check_binding` and refuse with all errors. Hash every bound input (SHA-256, streamed through root handles). Produce a `RunRecord` (run ID UUIDv7, program ID, version, digest, binding as authored, input digests, principal, trial and override flags, branch ref placeholder, outcome `pending`) in canonical JSON. Acceptance: the five cases in the spec. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; no unit conversion; ordered collections only.

## Open questions

- Build notes (fid-run): `prepare_run` takes the bus-derived `Principal`; input datasets are opened through a host-wired `InputSource` (the bus file policy and root handles), and the run ID comes from a host-wired `RunIds`. A `ProgramRef::Identified` program is read from the lifecycle store's frozen bytes. The trial and override flags are recorded as requested; the override lifts a refusal only on a deprecated program.
- Authority (REQ-CORE.RPL.RECON.FOLD-008): the crate defines the `RunStarted` event `{record}` and the origin-bound `RunLedger` fold with its commit-stage `precommit` (`crates/fid-run/src/ledger.rs`). The entry's parameters must be `run_params(record)` (program, digest, binding, every input handle, variant, both flags); the fold and the commit stage check the bus parameters commitment of the entry against them. Refusals are fatal, except promoted copies.
- Promoted copies are skipped, not refused: a run starts only on `run/<run_id>`, and promoting that branch copies its `RunStarted` entry with `promoted_from` set. The copy names a run already admitted (or refused) from its original entry, so it adds no run and no authority; refusing it would make every promotion of a run branch fail the fold.
- Deviation from FOLD-008 (d): two runs of one program decided against the same lifecycle state are both admitted, because a run consumes nothing (it changes no lifecycle state and no other run). A replay of one run is still refused by its run ID, and a run whose lifecycle state changed since it was prepared is `conflict/head_moved` at commit.
- A→B→A: lifecycle states move forward except `unregister` (registered → draft). That is harmless here: an unregistered version is retired, its number is never reused, and the same bytes come back only under a new version number, so a (version, state) a run was decided against never recurs once left.
- Fold domain (maintainer decision): the ledger folds all branches in commit order, and the lifecycle store it judges against is the project's lifecycle state (that of `main`) as of the entry, for both `precommit` and the fold, so they cannot disagree.
- Run branch (maintainer decision): a run starts on exactly `run/<run_id>`, and every other branch is refused. Obligation on `AUTO.RUN.EXEC.BRANCH` and `AUTO.RUN.EXEC.CMD` (not edited from here): `program.run@1` commits `RunStarted` on `run/<run_id>` after the fork, with parameters exactly `run_params(record)`; a front end resolves a variant's binding into those parameters, and the handler re-checks it.
- Single owner (maintainer decision): `RunStarted` is reserved to `program.run@1` alone. `AUTO.RUN.BATCH` must start each of its runs through `program.run@1`.
- `AUTO.PROG.FMT` has no per-step `retargetable` field yet (see `AUTO.REC.LIFT`), so `steps_retargetable` marks every step `true`.
- REQ-005's trial mark on results is carried by the record's `trial` flag; marking each result is `AUTO.RUN.EXEC`'s job.

## Children

_Leaf: none._
