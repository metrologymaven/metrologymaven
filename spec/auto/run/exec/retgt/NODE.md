---
id: AUTO.RUN.EXEC.RETGT
title: Not-re-targetable step guard
level: 4
parent: AUTO.RUN.EXEC
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
built_in: [fid-run]
satisfies: [REQ-AUTO.RUN.EXEC-002]
covers: []
depends_on: [AUTO.RUN.BIND]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# AUTO.RUN.EXEC.RETGT: Not-re-targetable step guard

## Purpose

Defines the `StepGate` pre-step hook trait that `AUTO.RUN.EXEC.CMD` calls before every step, and provides its first implementation, a guard that stops a re-targeted run before it applies a recorded selection to new data. Steps marked `retargetable: false` (set by the record-to-program lift when a selection could not be expressed as a semantic query) may run only when every bound input is byte-identical to the input recorded with the program. Otherwise the run stops at that step with `not_retargetable`.

## Scope

**In:**
- The `StepGate` trait and `GateDecision` type (re-exported by `AUTO.RUN.EXEC.CMD` for other gates)
- Deciding, per run, whether the run is re-targeted: any bound input digest differs from the recorded default's digest
- The gate check before each step listed `false` in `RunPlan.steps_retargetable`
- The typed stop result naming the step and the differing inputs

**Out** (and which node owns it instead):
- Marking steps `retargetable: false` (`AUTO.REC.LIFT`)
- Calling the gate, stopping the interpreter and finalising the record as failed (`AUTO.RUN.EXEC.CMD`)
- The `steps_retargetable` list and input digests (`AUTO.RUN.BIND`)

## Requirements

- **REQ-AUTO.RUN.EXEC.RETGT-001** The guard shall treat a run as re-targeted if the SHA-256 of any bound input differs from the digest of that parameter's recorded default, or if a bound input parameter has no recorded default.
- **REQ-AUTO.RUN.EXEC.RETGT-002** If a re-targeted run reaches a step listed `retargetable: false`, then the guard shall return `Stop(not_retargetable)` naming the step ID, iteration path and the differing input parameters, before the step is invoked.
- **REQ-AUTO.RUN.EXEC.RETGT-003** If a run is not re-targeted, then the guard shall allow every step, including those marked `retargetable: false`.
- **REQ-AUTO.RUN.EXEC.RETGT-004** The guard shall be a pure function of the run plan and step key, with no I/O.

## Acceptance criteria

- **AC-1** Given fixture program `fixtures/program/retarget/manual-pick.json` (step `s4` marked `retargetable: false`) bound to its recorded scan `fixtures/scans/bracket-120.ply`, when the guard is evaluated for every step, then every step is allowed.
- **AC-2** Given the same program bound to `fixtures/scans/bracket-120-shifted.ply`, then steps `s1`..`s3` are allowed and `s4` returns `Stop(not_retargetable)` naming `s4` and parameter `scan`.
- **AC-3** Given a binding whose input parameter has no recorded default, then the run is re-targeted and `s4` stops.
- **AC-4** Given the same plan and step key twice, then the two decisions are identical.

## Design

- **Registry command(s):** none.
- **Data structures:** `RetargetGuard { retargeted: bool, differing: Vec<String>, blocked: BTreeSet<StepId> }` built from a `RunPlan`; implements `StepGate::before_step(&StepKey, &StepInfo) -> GateDecision { Allow | Stop(code, detail) | Hold(reason) }` (it never returns `Hold`).
- **Algorithm:** at build time compare each bound input digest with the recorded default digest; at each step, `Stop` if `retargeted` and the step ID is in `blocked`.
- **Errors:** `not_retargetable` (as a stop decision).
- **Determinism:** D0.
- **Performance budget:** O(log n) per step [unverified until measured].
- **Crate:** `fid-run` (module `exec::retarget`)

## Test fixtures

New `fixtures/program/retarget/manual-pick.json` (four steps, `s4` not re-targetable, recorded default scan `bracket-120.ply`); scans `bracket-120.ply` and `bracket-120-shifted.ply` from the seeded synthetic generator (shared). Project-authored.

## Build prompt

> In crate `fid-run`, add module `exec::retarget` with `RetargetGuard::from_plan(&RunPlan)` and define in the same module the trait `StepGate { fn before_step(&self, key: &StepKey, info: &StepInfo) -> GateDecision; fn after_step(&mut self, key: &StepKey, result: &StepResult) {} ; fn before_end(&self) -> GateDecision { Allow } }` (the last two with default no-op bodies) with `GateDecision { Allow, Stop(code, detail), Hold(reason) }`; `RetargetGuard` implements it. A run is re-targeted when any bound input's SHA-256 differs from its parameter's recorded default digest, or the parameter has no recorded default. For a re-targeted run, return `Stop(not_retargetable)` before any step listed `retargetable: false` in `RunPlan.steps_retargetable`, naming the step ID, iteration path and the differing parameters; otherwise `Allow`. Pure, no I/O. Acceptance: AC-1..AC-4 with `fixtures/program/retarget/manual-pick.json`. Constraints: CONSTITUTION §8 (no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`); `BTreeSet`/`BTreeMap` only.

## Open questions

- Recorded default digests (settled in the build, `fid-run`): `RunPlan.default_digests` holds the SHA-256 of the default dataset of every `data_handle` parameter that declares a default. `prepare_run` (`AUTO.RUN.BIND`) hashes it through the same `InputSource` as the bound inputs; a default that cannot be read is left out, not an error, so binding that parameter re-targets the run (fail closed). A program stores only the default's handle, so the "recorded" default is the dataset that handle names when the run is prepared. If that dataset is overwritten with new data, the guard cannot see it. Obligation on `AUTO.PROG.FMT` / `AUTO.REC.LIFT` (not edited from here): store the recorded input's digest with the default, and `prepare_run` should then compare against it.
- Marking (open, owned by `AUTO.REC.LIFT`, needs an additive `AUTO.PROG.FMT` field): no stored program can mark a step `retargetable: false` yet, so `prepare_run` lists every step re-targetable and the guard allows every step of a real plan. The fixture `fixtures/program/retarget/manual-pick.json` is a valid FMT program; its marking of `s4` lives in `fixtures/program/retarget/expected.json` (`not_retargetable`), and the tests apply it to the plan in LIFT's place. AC-1..AC-4 are met for the guard; the end-to-end path from a stored mark to the plan waits on LIFT.
- Step types (provisional until `AUTO.PROG.INTERP.ENGINE` is built): `exec::retarget` defines `StepKey { step_id, iteration_path }`, `StepInfo { command, class }` and `StepResult { command, output }`; ENGINE's `StepKey` should be this type or convert to it one-to-one. `GateDecision` is `Allow | Stop(code, detail) | Hold(reason)`; the `not_retargetable` detail is `{"iteration_path", "params", "step_id"}` (`NotRetargetable`).
- Authority: the guard emits no journal event and decides nothing that a fold replays, so REQ-CORE.RPL.RECON.FOLD-008 has nothing to bind here. `AUTO.RUN.EXEC.CMD` journals the stop and finalises the run `failed`; that entry is under `AUTO.RUN.EXEC.BRANCH`'s Run entity fold obligation.

## Children

_Leaf: none._
