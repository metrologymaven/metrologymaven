---
id: KRN.SOLV.MSTART
title: Deterministic seeded multi-start and agreement check
level: 3
parent: KRN.SOLV
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-solv]
satisfies: [REQ-KRN.SOLV-001, REQ-KRN.SOLV-008]
covers: []
depends_on: [KRN.NUM.RNG, KRN.NUM.FN]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# KRN.SOLV.MSTART: Deterministic seeded multi-start and agreement check

## Purpose

Non-convex fits can converge to a local optimum. This module runs a solve from a deterministic, seeded set of starts, compares the outcomes, picks the best by a fixed rule, and sets `local_optimum_only` when the starts disagree. It is generic over the solve, so least-squares (via NLLS) and minimax (via SEQ) callers share it.

## Scope

**In:** start-set generation (caller's primary start plus seeded perturbations on a caller-defined parameter box, or caller-listed starts); sequential execution in a fixed order; best-result selection with a total-order tie-break; agreement check; the start list and seed in the output for journaling.
**Out:** the solves themselves (`KRN.SOLV.NLLS`, `KRN.SOLV.SEQ`); problem-specific start heuristics (`KRN.LSQ.INIT`).

## Requirements

- **REQ-KRN.SOLV.MSTART-001** Given a primary start, a parameter perturbation box, a start count k and a seed, the module shall generate the same k starts on every target using only the `fid-num` counter-based RNG.
- **REQ-KRN.SOLV.MSTART-002** The module shall run the caller's solve closure on each start in index order, single-threaded, and collect each outcome or typed error.
- **REQ-KRN.SOLV.MSTART-003** The module shall select the successful outcome with the lowest objective, breaking ties by total order on the parameter vector and then by start index.
- **REQ-KRN.SOLV.MSTART-004** The module shall set `agreement = true` only if every successful outcome's parameters agree with the selected one within the caller-supplied per-parameter tolerances (default length 1e-9 m and angle 1e-9 rad [unverified]), and shall otherwise set `local_optimum_only`.
- **REQ-KRN.SOLV.MSTART-005** If every start fails, then the module shall return a typed error listing each start's error, and no value.
- **REQ-KRN.SOLV.MSTART-006** The output shall include the seed, the start list and the per-start outcome summary so the caller can journal them.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/solv/mstart-double-well-v1` (a 2-parameter function with two minima of different depth, solve closure = NLLS), seed 42 and k = 8, when the module runs, then it selects the deeper minimum, sets `local_optimum_only` (because some starts land in the other well), and the start list digest equals the golden digest.
- **AC-2** Given a convex quadratic with k = 8, then `agreement = true` and no flag.
- **AC-3** Given AC-1 on x86-64, ARM64 and wasm32 under wasmtime, then all outputs are bit-identical.
- **AC-4** (error path) Given a closure that fails for every start, then the module returns `MultiStartError::AllFailed` with 8 entries.
- **AC-5** Given two outcomes with equal objective, then the one with the lower total-order parameter vector is selected on every run.

## Design

- **Registry command(s):** none; library API `fid_solv::mstart::{run, StartSpec, MultiStartOutcome}`.
- **Data structures:** `StartSpec { primary: Vec<f64>, half_widths: Vec<f64>, count: u32, seed: u64, stream: u64, tolerances: Vec<f64> }`; `MultiStartOutcome<T> { best: T, best_index: u32, agreement: bool, local_optimum_only: bool, starts: Vec<Vec<f64>>, per_start: Vec<StartSummary> }`.
- **Algorithm:** start 0 is the primary; start i >= 1 is primary + (2u - 1) * half_widths with u from `Cursor{seed, stream, index}` drawn in parameter order. Selection and agreement as in the requirements.
- **Errors:** `MultiStartError::{AllFailed(Vec<SolverError>), InvalidSpec}`.
- **Determinism:** D0.
- **Performance budget:** overhead beyond the solves < 1 % [unverified until measured].
- **Crate:** `fid-solv` (module `mstart`).

## Test fixtures

`fixtures/solv/mstart-double-well-v1`: an analytic test function defined in the fixture file with golden digests, CC-BY-4.0.

## Build prompt

> In crate `fid-solv`, create module `mstart` with `pub fn run<T, F>(spec: &StartSpec, solve: F) -> Result<MultiStartOutcome<T>, MultiStartError> where F: FnMut(&[f64]) -> Result<(T, f64 /*objective*/, Vec<f64> /*params*/), SolverError>`. Starts: index 0 = primary; others = primary + (2u-1)*half_width per parameter, u from `fid_num::rng::Cursor` (seed, stream, sequential index). Run in index order, single-threaded. Select lowest objective; ties by `fid_num::f::total_cmp` over the parameter vector, then start index. `agreement` iff every successful outcome is within per-parameter tolerances of the best; otherwise set `local_optimum_only`. Return starts and per-start summaries for journaling. Tests: `fixtures/solv/mstart-double-well-v1` (golden digest, deeper well selected, flag set), convex case agreement, cross-target bit identity (x86-64, ARM64, wasm32), all-fail error, deterministic tie-break. KNP-1: no FMA, no `f64::min`/`max`, no HashMap/HashSet, no threads, no `unsafe`, no I/O; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-5 of spec node KRN.SOLV.MSTART.

## Open questions

- Default agreement tolerances should come from the REP-1 table once the constitution adopts it.

## Children

_Leaf: none._
