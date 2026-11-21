---
id: ALN.QA.HIST
title: Convergence history and published convergence criteria
level: 3
parent: ALN.QA
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-align-solve, fid-xtask]
built_pr: 396
satisfies: [REQ-ALN.QA-003, REQ-ALN.QA-004]
covers: []
depends_on: [CORE.REG, KRN.NUM]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ALN.QA.HIST: Convergence history and published convergence criteria

## Purpose

Two things every iterative alignment method must provide: a per-iteration convergence history stored with the alignment, and a machine-readable declaration of its convergence criteria and defaults in the registry manifest, from which the documentation is generated. A CI check refuses any iterative method that lacks the declaration, which is how the documentation half of REQ-ALN-003 is kept true.

## Scope

**In:**
- `ConvergenceHistory` type and its recorder API
- The registry-manifest `convergence` block schema for iterative methods
- The CI check and the generated documentation fragment

**Out** (and which node owns it instead):
- Stopping logic (`ALN.BFIT.STOP` and each iterative solver)
- Exposure through `align.get_diagnostics@1` (`ALN.QA.COND`)
- The documentation site and its generator pipeline (`CORE.REG`, `OUTR.DOCS`); this leaf supplies the fragment

## Requirements

- **REQ-ALN.QA.HIST-001** For every iterative method, the alignment shall store a history with, per iteration, the iteration number, objective value, translation increment (m), rotation increment (rad) and number of inputs used, plus the stopping criterion that fired.
- **REQ-ALN.QA.HIST-002** The recorder shall keep every iteration up to 100,000 and shall fail with `history_overflow` beyond that.
- **REQ-ALN.QA.HIST-003** Every registry command whose algorithm is marked `iterative` shall declare a `convergence` block listing each criterion's name, unit, default and one-line meaning.
- **REQ-ALN.QA.HIST-004** If an `iterative` command lacks the block or a criterion lacks a unit or default, then the CI check shall fail naming the command and field.
- **REQ-ALN.QA.HIST-005** The documentation generator shall render each command's convergence block as a table, and the rendered text shall be produced from the manifest only, never edited by hand.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/aln/qa/hist-manifest` (a test registry with three iterative commands, one missing a default), when the CI check runs, then it fails naming that command and `default`; after the fix, it passes.
- **AC-2** Given a recorder fed 12 scripted iterations ending in `converged(increment)`, then the stored history has 12 rows with the given values and the stopping criterion.
- **AC-3** (error path) Given 100,001 iterations, then the recorder fails with `history_overflow`.
- **AC-4** Given the fixture manifest, then the generated fragment is byte-identical on two runs and contains one table per iterative command.

## Design

- **Commands:** none; the history is read through `align.get_diagnostics@1`.
- **Data structures:** `ConvergenceHistory { rows: columnar (iteration, objective, d_translation, d_rotation, n_used), stopped_by }`; manifest block `convergence: [{ name, unit, default, meaning }]` (meaning at most 120 characters).
- **Algorithm:** append-only recorder; CI check walks the registry manifest in sorted command-ID order; fragment rendered as Markdown tables in that order.
- **Errors:** `history_overflow`, `manifest_convergence_missing`, `manifest_field_missing`.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-align-solve` (history type) and a registry check in the registry tooling crate (provisional names).

## Test fixtures

`fixtures/aln/qa/hist-manifest`: a hand-authored test registry manifest with three iterative commands (one defective) and scripted iteration traces. Project-authored, CC-BY-4.0.

## Build prompt

> Implement convergence history and criteria publication for iterative alignment methods.
> In kernel crate `fid-align-solve`: `ConvergenceHistory` (per iteration: number, objective, translation increment in m, rotation increment in rad, inputs used; plus the stopping criterion) with an append-only recorder that fails with `history_overflow` beyond 100,000 iterations.
> In the registry tooling: a manifest schema block `convergence: [{ name, unit, default, meaning }]` required for every command whose algorithm is marked `iterative`; a CI check that fails naming the command and missing field; and a documentation fragment generator that renders one table per iterative command in sorted command-ID order, generated only from the manifest.
> Acceptance: fixture `fixtures/aln/qa/hist-manifest`; tests: defective manifest fails the check by name and passes after the fix; 12-iteration recorder contents; overflow error; byte-identical generated fragment.
> Constraints: no panic, no unwrap/expect, no unsafe; no std HashMap/HashSet; generated docs are never hand-edited (CONSTITUTION §8 honest status).

## Open questions

- None blocking.
- **As built:** the history is `fid_align_solve::hist` (`ConvergenceHistory`, `HistoryRecorder`); `finish` takes the final pose as an ALN.BFIT.CONSTR `CertifiedPose`, never a bare `Rigid`. Iteration numbers must run 1, 2, 3, ... (`history_bad_iteration`), values must be finite (`history_non_finite`) and increments non-negative (`history_negative_increment`), beyond the spec's `history_overflow`. The check and fragment are `cargo xtask convergence-check` / `convergence-doc` (`fid-xtask`, module `convergence`); a malformed block value is `manifest_field_invalid`, a repeated command `manifest_duplicate_command`.
- **Needs CORE.REG.DEF:** `CommandDef` has no `iterative` marking and no `convergence` block yet, so the check reads them from a manifest-shaped file (as `registry-gate` reads its current declarations). Proposed: CORE.REG.DEF adds both fields to the definition and its manifest form, and CI then runs `convergence-check` on the exported workspace manifest and `convergence-doc --check` on the committed docs fragment. No iterative command exists on main yet.

## Children

_Leaf: none._
