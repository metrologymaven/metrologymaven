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
- **Errors:** `not_released`, `deprecated`, `binding_invalid`, `input_unreadable`.
- **Determinism:** D0 (apart from the run ID).
- **Performance budget:** dominated by hashing inputs; ≥ 500 MB/s on reference hardware [unverified until measured].
- **Crate:** `fid-run`

## Test fixtures

`fixtures/program/family/bracket.json` (shared), `fixtures/scans/bracket-120.ply` (synthetic, seeded generator, project-authored); lifecycle states set through the CORE test harness.

## Build prompt

> In a new crate `fid-run`, implement `prepare_run(program_ref, binding_or_variant, principal, flags)`. Rules: released programs run for any allowed principal; draft or registered only as a human trial run (`not_released` otherwise, record `trial: true`); deprecated only with a human override (`deprecated` otherwise, record the override). Validate the binding with `fid_program::check_binding` and refuse with all errors. Hash every bound input (SHA-256, streamed through root handles). Produce a `RunRecord` (run ID UUIDv7, program ID, version, digest, binding as authored, input digests, principal, trial and override flags, branch ref placeholder, outcome `pending`) in canonical JSON. Acceptance: the five cases in the spec. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; no unit conversion; ordered collections only.

## Open questions

- Build notes (fid-run): `prepare_run` takes the bus-derived `Principal`; the input datasets are opened through a host-wired `InputSource` (the bus file policy and root handles) and the run ID comes from a host-wired `RunIds`. A `ProgramRef::Identified` program is read from the lifecycle store's frozen bytes; an unknown digest is `program_not_found` (a code not in the Errors list above).
- Authority (REQ-CORE.RPL.RECON.FOLD-008): the crate also defines the `RunStarted` event `{params_commitment, record}` that `AUTO.RUN.EXEC` commits and the origin-bound `RunLedger` fold with its commit-stage `precommit`. `RunStarted` is reserved to `program.run@1`; `AUTO.RUN.BATCH` must start each run through that command, or a later change adds a second owner. Rules, branch rule (every branch counts, a run never starts on `main`, promoted copies are skipped) and refusal policy (fatal) are in `crates/fid-run/src/ledger.rs`.
- `AUTO.PROG.FMT` has no per-step `retargetable` field yet (see `AUTO.REC.LIFT`), so `steps_retargetable` marks every step `true`.
- REQ-005's trial mark on results is carried by the record's `trial` flag; marking each result is `AUTO.RUN.EXEC`'s job.

## Children

_Leaf: none._
