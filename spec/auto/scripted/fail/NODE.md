---
id: AUTO.SCRIPTED.FAIL
title: Recompute guard: class limits, time limit and staleness
level: 3
parent: AUTO.SCRIPTED
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.SCRIPTED-005, REQ-AUTO.SCRIPTED-006]
covers: []
depends_on: [AUTO.SCRIPTED.DECL, CORE.BUS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.SCRIPTED.FAIL: Recompute guard: class limits, time limit and staleness

## Purpose

The guard around every scripted recompute. It runs the implementation under an automation principal that may call only R0 and R1 commands (stricter than the parent's deny list, so a recompute can never commit, export, touch a device or move a machine), enforces the declared time limit, and on any failure marks the element failed with the error and its dependents stale, never computed from partial output.

## Scope

**In:**
- `RecomputeGuard::run(decl, inputs, runner) -> GuardOutcome` over a `ScriptRunner` trait (Python host or interpreter)
- Class allowlist R0 and R1 for calls made during recompute
- Time limit from the declaration (default 60 s [unverified]); cancellation of the runner on expiry
- Outcome mapping: ok with outputs, failed with typed error; dependents marked stale on failure
- Output shape check against declared outputs

**Out:**
- Process isolation (`AUTO.PY.HOST`); recompute scheduling (`AUTO.SCRIPTED.GRAPH`, `CORE.DEP`)

## Requirements

- **REQ-AUTO.SCRIPTED.FAIL-001** During a recompute, any command call from the implementation whose class is not R0 or R1 shall be refused with `not_allowed_in_recompute`, and the recompute shall fail.
- **REQ-AUTO.SCRIPTED.FAIL-002** If the implementation runs longer than the declared time limit, then the guard shall cancel it and return failed with `time_limit`.
- **REQ-AUTO.SCRIPTED.FAIL-003** If the implementation errors, crashes or returns outputs that do not match the declared output names and types, then the guard shall return failed with the typed error and no outputs.
- **REQ-AUTO.SCRIPTED.FAIL-004** When the guard returns failed, the element shall be marked `failed` with the error and every dependent shall be marked `stale`, and no dependent shall be computed from the failed element's previous or partial outputs.
- **REQ-AUTO.SCRIPTED.FAIL-005** If the implementation artifact is deprecated at recompute time, then the guard shall return failed with `implementation_deprecated` unless a human has pinned it.

## Acceptance criteria

- **AC-1** Given the fake runner script `ok` returning declared outputs within 1 s, when the guard runs, then outcome ok with exactly those outputs.
- **AC-2** (error path) Given fake runner `calls-w2` that invokes a W2 command, then the call is refused with `not_allowed_in_recompute`, the fake bus records no commit, and the outcome is failed.
- **AC-3** (error path) Given fake runner `sleeps` with time limit 2 s, then failed with `time_limit` after no more than 2.5 s and the runner received cancel.
- **AC-4** (error path) Given fake runner `wrong-outputs`, then failed with `output_mismatch`.
- **AC-5** Given a failed outcome on element E with dependents D1 and D2 in the CORE.DEP test graph, then E is `failed`, D1 and D2 are `stale`, and neither recomputed.
- **AC-6** Given each fake runner twice, then outcomes are identical (timing excluded).

## Design

- **Registry command(s):** none.
- **Data structures:** `trait ScriptRunner { fn start(&self, payload) -> RunnerHandle; fn cancel(&self, h); }`; `GuardOutcome { Ok(Outputs) | Failed(GuardError) }`; `GuardError { NotAllowedInRecompute, TimeLimit, OutputMismatch, ImplementationError(String), ImplementationDeprecated }`.
- **Algorithm:** bind a recompute principal whose class set is {R0, R1}; the bus enforces it (the guard also pre-checks for a clear error); deadline timer; output validation against declarations.
- **Errors:** as listed.
- **Determinism:** D0 for the outcome given the runner's behaviour; the time limit itself is wall-clock.
- **Performance budget:** guard overhead < 1 ms per recompute [unverified until measured].
- **Crate:** `fid-scripted`

## Test fixtures

Fake runners `ok`, `calls-w2`, `sleeps`, `wrong-outputs` in `fid-scripted` tests; CORE.DEP test graph from the CORE harness.

## Build prompt

> In crate `fid-scripted`, implement `RecomputeGuard::run(decl, inputs, runner)` over a `ScriptRunner` trait. Run the implementation under a recompute principal allowed only R0 and R1 commands; refuse anything else with `not_allowed_in_recompute` and fail the recompute. Enforce the declaration's time limit (default 60 s) by cancelling the runner (`time_limit`). Validate outputs against declared names and types (`output_mismatch`). Fail with `implementation_deprecated` if the artifact is deprecated and not pinned by a human. On failure, mark the element `failed` with the error and all dependents `stale` through the CORE.DEP API, never computing them from previous or partial outputs. Acceptance: the six cases with fake runners and the CORE.DEP test graph. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; C1 (no values of record computed here).

## Open questions

- The R0/R1 allowlist is stricter than REQ-AUTO.SCRIPTED-005's deny list (which would still allow W1 and W2). A recompute that commits W2 commands mid-pass would break CORE.DEP's single-entry rule, so the stricter rule is proposed; reviewers may relax it.

## Children

_Leaf: none._
