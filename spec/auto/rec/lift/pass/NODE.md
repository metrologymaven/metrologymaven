---
id: AUTO.REC.LIFT.PASS
title: Data-handle lifting pass, lift report and lift command
level: 4
parent: AUTO.REC.LIFT
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.REC.LIFT-001, REQ-AUTO.REC.LIFT-002, REQ-AUTO.REC.LIFT-004]
covers: [GOM.AUT.02]
depends_on: [AUTO.REC.CONV, AUTO.REC.ADOPT, AUTO.PROG.FAMILY, AUTO.PROG.EXPR.GRAMMAR, CORE.REG, CORE.RPL]
side_effect: W1
determinism: D0
cli: [fid program lift-parameters]
mcp: [program_lift_parameters]
journal: [program.lift_parameters@1]
updated: 2026-11-17
---

# AUTO.REC.LIFT.PASS: Data-handle lifting pass, lift report and lift command

## Purpose

The first generalisation of a recorded draft: every input data handle the session used (the scan, CAD or measurement file it imported, the project inputs it read) becomes a declared program parameter whose default is the recorded handle, and every use is rewritten into a reference to that parameter. The pass runs automatically after `program.from_journal@1` as a `DraftPass`, and again on demand through `program.lift_parameters@1`. It produces the lift report that lists everything left literal, and it guarantees that the lifted draft still replays `IDENTICAL` with its default binding.

## Scope

**In:**
- Registration as a `DraftPass` with `AUTO.REC.CONV`
- Identification of data-handle parameters from the manifest parameter types (input root handles and project input references)
- Parameter naming (`input_1`, `input_2`, ... or the import command's declared role name), deduplication of the same handle used by several steps
- Rewriting each use into `{"expr": "<param name>"}`; declaring the parameter through `AUTO.PROG.FAMILY` types with the recorded handle as default
- The lift report: lifted parameters, literal arguments left unlifted (step ID, parameter name), untranslated selections from `AUTO.REC.LIFT.SELECT`
- `program.lift_parameters@1` (W1 on drafts) and the replay-identity check

**Out:**
- Lifting chosen nominal values and element references (`AUTO.REC.LIFT.VALUES`)
- Selection translation (`AUTO.REC.LIFT.SELECT`)
- Parameter binding checks (`AUTO.PROG.FAMILY`)

## Requirements

- **REQ-AUTO.REC.LIFT.PASS-001** The pass shall turn every distinct input data handle in the draft's literal arguments into one declared program parameter whose default is the recorded handle, and rewrite every use into a reference to that parameter.
- **REQ-AUTO.REC.LIFT.PASS-002** The lift report shall list every literal argument left unlifted with step ID and parameter name, ordered by step position.
- **REQ-AUTO.REC.LIFT.PASS-003** When the pass completes, it shall replay the lifted draft with its default binding in exact mode against the range's start state, and if the outcome is not `IDENTICAL`, then it shall discard the lifting, keep the unlifted draft and report `lift_not_identical` with the first divergent step.
- **REQ-AUTO.REC.LIFT.PASS-004** `program.lift_parameters@1` shall apply only to drafts, shall be idempotent (running it twice gives the same draft digest as running it once), and shall refuse non-drafts with `immutable`.
- **REQ-AUTO.REC.LIFT.PASS-005** Each step the pass rewrites shall be recorded with `record_authorship` under the principal of the command that ran the pass.

## Acceptance criteria

- **AC-1** Given fixture project `fixtures/projects/align-and-check/` and the draft from `program.from_journal@1` over its 12 entries (two imports of distinct files, one read by three steps), when the pass runs, then the draft declares two parameters with the recorded handles as defaults, four arguments are rewritten, and the lift report lists the remaining literals.
- **AC-2** (replay) Given the lifted draft and its default binding, when replayed in exact mode from the snapshot at `e1`, then the outcome is `IDENTICAL`.
- **AC-3** Given `fid program lift-parameters --program P --json` run twice, then both draft digests are equal.
- **AC-4** (error path) Given a fake replay that diverges at step S5, then the lifting is discarded, the draft digest equals the unlifted one and the report holds `lift_not_identical` at S5.
- **AC-5** (error path) Given a registered program, then `immutable` and nothing changes.
- **AC-6** Given the pass run by an MCP agent session, then every rewritten step is unadopted for that agent.

## Design

- **Registry command(s):** `program.lift_parameters@1`, W1 on drafts. Params: `program: ProgramId`, `expected_digest`. Result: `LiftReport { lifted: Vec<{param, default, uses: Vec<StepId>}>, unlifted: Vec<{step_id, param}>, untranslated_selections: Vec<StepId>, outcome: Lifted | NotIdentical{step} }`.
- **Data structures:** `DraftPass` trait from `AUTO.REC.CONV`; parameter declarations from `AUTO.PROG.FAMILY`.
- **Algorithm:** scan invoke arguments in step order; match parameter types flagged as data handles in the manifest; assign names; rewrite; run SELECT and VALUES rules if registered; replay check through `CORE.RPL`.
- **Errors:** `immutable`, `stale_program`, `lift_not_identical` (reported, not fatal), `program_not_found`.
- **Determinism:** D0.
- **Performance budget:** pass over 10 000 steps < 200 ms excluding the replay check [unverified until measured].
- **Crate:** `fid-program-rec` (module `lift::pass`).

## Test fixtures

`fixtures/projects/align-and-check/` (seeded synthetic project from `AUTO.REC.CONV`); a fake replay engine that can diverge at a chosen step.

## Build prompt

> In crate `fid-program-rec`, add module `lift::pass`: a `DraftPass` registered with `program.from_journal@1`, and the registry command `program.lift_parameters@1` (W1, drafts only; CLI `fid program lift-parameters`, MCP `program_lift_parameters`). Find literal arguments whose manifest parameter type is a data handle (input root handle or project input reference); declare one program parameter per distinct handle (default = recorded handle, type from the registry); rewrite each use to `{"expr": "<name>"}`; record authorship for rewritten steps with the caller's principal. Build the lift report (lifted, unlifted literals by step and parameter, untranslated selections). Replay the lifted draft with its default binding in exact mode; if not `IDENTICAL`, discard the lifting and report `lift_not_identical` with the first divergent step. Idempotent; `immutable` on non-drafts. Acceptance: AC-1..AC-6. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; never change a released program (C5, REC lifecycle).

## Open questions

- Which manifest parameter types count as "data handles" needs a flag in `CORE.REG.DEF` (for example `input_handle: true`); until then the pass recognises the root-handle type and project input reference types by name.

## Children

_Leaf: none._
