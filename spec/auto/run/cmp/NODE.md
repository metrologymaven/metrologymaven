---
id: AUTO.RUN.CMP
title: Run comparison against a reference run
level: 3
parent: AUTO.RUN
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.RUN-003]
covers: []
depends_on: [AUTO.RUN.BIND, CORE.RPL]
side_effect: R1
determinism: D0
cli: [fid program compare-runs]
mcp: [program_compare_runs]
journal: [program.compare_runs@1]
updated: 2026-11-16
---

# AUTO.RUN.CMP: Run comparison against a reference run

## Purpose

`program.compare_runs@1`: compares a run's journal branch with a chosen reference run of the same program, step by step, and reports for each step `IDENTICAL`, `EQUIVALENT`, `DIVERGENT` or `NOT_REPLAYABLE` using CORE's replay classification rules, so "did the new part go through the same inspection with the same decisions?" has one answer.

## Scope

**In:**
- Pairing of journal entries by program step ID (and loop iteration index)
- Classification per pair with the CORE.RPL rules (digest equality, quantity-kind tolerance, decision changes)
- Unpaired steps (control flow diverged) and steps with missing inputs
- Summary: counts per outcome, first divergent step

**Out:**
- The classification rules themselves (`CORE.RPL`)
- Statistics across many runs (`RPT.STATS`)

## Requirements

- **REQ-AUTO.RUN.CMP-001** The command shall pair the two runs' entries by step ID and iteration index, and shall classify each pair with the CORE replay rules.
- **REQ-AUTO.RUN.CMP-002** If a step ran in only one of the two runs, then it shall be reported `DIVERGENT` with reason `control_flow`.
- **REQ-AUTO.RUN.CMP-003** If any conformance decision differs between paired entries, then that step shall be `DIVERGENT` regardless of numeric closeness.
- **REQ-AUTO.RUN.CMP-004** If a recorded output needed for comparison is missing, then that step shall be `NOT_REPLAYABLE` naming the missing item.
- **REQ-AUTO.RUN.CMP-005** If the runs are of different program digests, then the command shall refuse with `different_programs`.

## Acceptance criteria

- **AC-1** Given fixture project `fixtures/projects/bracket-runs/` with runs R1 and R2 of the same released program on the same scan, when `fid program compare-runs --run R2 --reference R1 --json` runs, then every step is `IDENTICAL`.
- **AC-2** Given run R3 on scan `bracket-120-shifted.ply` (0.5 µm shift, all decisions unchanged), then measurement steps are `EQUIVALENT` and no step is `DIVERGENT`.
- **AC-3** Given run R4 where one characteristic flips from CONFORMS to NONCONFORMS, then that step is `DIVERGENT` and the summary names it as the first divergent step.
- **AC-4** Given run R5 where a branch took the else path, then the steps of the skipped path are `DIVERGENT` with `control_flow`.
- **AC-5** (error path) Given runs of two different programs, then `different_programs`.
- **AC-6** Given the same two runs twice, then the reports are byte-identical.

## Design

- **Registry command(s):** `program.compare_runs@1`, class R1; params `run: RunId`, `reference: RunId`; result `{ steps: [{step_id, iteration, outcome, reason}], summary: {counts, first_divergent} }`.
- **Data structures:** `StepKey { step_id, iteration_path: Vec<u32> }`.
- **Algorithm:** index both branches by `StepKey` (entries carry step ID and iteration path in their metadata, written by the interpreter); merge-join in program order; call the CORE.RPL classifier per pair.
- **Errors:** `different_programs`, `run_not_found`.
- **Determinism:** D0.
- **Performance budget:** 10,000-step runs compared in < 1 s [unverified until measured].
- **Crate:** `fid-run`

## Test fixtures

`fixtures/projects/bracket-runs/` built by a seeded generator: one released program and runs R1 to R5 as described (project-authored).

## Build prompt

> In crate `fid-run`, implement registry command `program.compare_runs@1` (R1; CLI `fid program compare-runs`, MCP `program_compare_runs`). Index the journal entries of both runs' branches by (step ID, iteration path), merge-join in program order, and classify each pair with the CORE.RPL classifier (IDENTICAL, EQUIVALENT within quantity-kind tolerance, DIVERGENT on any decision change, NOT_REPLAYABLE with the missing item). Steps present in one run only are DIVERGENT with reason `control_flow`. Refuse runs of different program digests (`different_programs`). Return per-step outcomes plus counts and the first divergent step. Acceptance: the six cases on fixture project `bracket-runs`. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; no own tolerance rules (use CORE's); ordered collections only.

## Open questions

- Pairing relies on the interpreter writing step ID and iteration path into each entry's metadata; this is a requirement on `AUTO.PROG.INTERP` to be added when it is split.

## Children

_Leaf: none._
