---
id: ALN.PRE.GLOBAL.CMD
title: Prealignment command, R1 pipeline entry and alignment wrapping
level: 4
parent: ALN.PRE.GLOBAL
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-ALN.PRE.GLOBAL-001, REQ-ALN.PRE.GLOBAL-002, REQ-ALN.PRE.GLOBAL-004, REQ-ALN.PRE.GLOBAL-006]
covers: [GOM.ALN.01]
depends_on: [ALN.PRE.GLOBAL.SAMPLE, ALN.PRE.GLOBAL.HYPO, ALN.PRE.GLOBAL.SCORE, ALN.PRE.AMBIG, ALN.MODEL.ELEM, ALN.QA.REPORT]
side_effect: W2
determinism: D2
cli: [fid align run-prealign]
mcp: [align_run_prealign]
journal: [align.run_prealign@1]
updated: 2026-11-17
---

# ALN.PRE.GLOBAL.CMD: Prealignment command, R1 pipeline entry and alignment wrapping

## Purpose

The registry command `align.run_prealign@1` (GOM automatic prealignment) and the pure pipeline `prealign(measured, reference, params) -> CandidateSet` that SCAN also calls: sample, generate hypotheses, score, filter through `ALN.PRE.AMBIG`, and create the alignment from the candidate AMBIG returns, flagged `ambiguous` when it reports so.

## Scope

**In:** parameter validation; the R1 pipeline composition; the AMBIG call with hints and optional `candidate_index`; alignment creation with seed, voxel size, K, method ID and candidate list; status `ambiguous`; dry-run.
**Out:** the three pipeline stages (siblings); hint and ambiguity logic (`ALN.PRE.AMBIG`); corpus and benchmark (`ALN.PRE.CORPUS`); picked pairs (`ALN.PRE.PAIRS`).

## Requirements

- **REQ-ALN.PRE.GLOBAL.CMD-001** When `align.run_prealign` commits, the system shall create one alignment from AMBIG's chosen candidate without any user-picked point, recording seed, voxel size, K, methods, the hypothesis algorithm ID of the chosen candidate, hints digest and the ranked candidate list, and journal one event.
- **REQ-ALN.PRE.GLOBAL.CMD-002** The command shall pass the top K candidates with scores and inlier fractions to `ALN.PRE.AMBIG`; when AMBIG returns `ambiguous`, the alignment shall be created with status `ambiguous` from the best candidate and list all ambiguous candidates.
- **REQ-ALN.PRE.GLOBAL.CMD-003** The `seed` parameter shall default to 0, always be recorded, and identical inputs, parameters and seed shall give a bit-identical transform (registry class D2).
- **REQ-ALN.PRE.GLOBAL.CMD-004** Errors from any stage (including `insufficient_data`, `no_candidate_matches_hints`, `candidate_out_of_range`) shall pass through, and the command shall then create and journal nothing.
- **REQ-ALN.PRE.GLOBAL.CMD-005** With `--dry-run`, the command shall run as R1 and return the candidate set, journaling nothing.
- **REQ-ALN.PRE.GLOBAL.CMD-006** The R1 function `prealign` shall return the same candidate set as the command's dry-run for the same inputs.

## Acceptance criteria

- **AC-1** Given project fixture `fixtures/aln/pre/global/bracket_random_pose.fidproj` (a corpus part at a random pose), when `fid align run-prealign --measured cloud --reference cad --seed 1 --json` runs, then one alignment is created within 2 mm and 2 degrees of the true pose and one event is journaled.
- **AC-2** Given a symmetric part without a symmetry hint, then the alignment has status `ambiguous` and lists the candidates.
- **AC-3** Given `--candidate-index 1` on AC-2, then the second candidate is used and outcome `chosen_by_user` is recorded.
- **AC-4** (error path) Given 50 measured points, then the command fails with `insufficient_data` and journals nothing.
- **AC-5** (replay) Exact replay of AC-1 gives `IDENTICAL`.

## Design

- **Registry command:** `align.run_prealign@1` (W2; dry-run R1; D2). Params: `measured`, `reference`, `seed: u64 = 0`, `k_candidates: 1..=64 = 8`, `hints?: PrealignHints`, `candidate_index?`, `sampling?: { voxel_size: Length }`, `parent?`. Result: `{ alignment, status, candidates }`.
- **Function:** `prealign(measured, reference, params) -> Result<CandidateSet, PrealignError>` (R1, for SCAN).
- **Algorithm:** sample both sides; generate; score; `filter_candidates` (AMBIG); wrap through `ALN.MODEL.ELEM`; residual report from the chosen candidate's scoring distances.
- **Errors:** stage errors passed through; `element_not_found`, `unsupported_reference`.
- **Determinism:** D2.
- **Performance budget:** end to end under 10 s for 1 M points and 1 M triangles on reference hardware [unverified], timed by `ALN.PRE.CORPUS`.
- **Crate:** `fid-align` (command layer and the `prealign` entry; provisional name).

## Test fixtures

`bracket_random_pose.fidproj` and a symmetric cylinder part from `ALN.PRE.CORPUS` (CC0).

## Build prompt

> In crate `fid-align`, implement the R1 function `prealign(measured, reference, params)` composing `sample` (ALN.PRE.GLOBAL.SAMPLE), `generate` (ALN.PRE.GLOBAL.HYPO), `score` (ALN.PRE.GLOBAL.SCORE) and `filter_candidates` (ALN.PRE.AMBIG). Register `align.run_prealign@1` (W2, dry-run R1, D2; CLI `fid align run-prealign`; MCP `align_run_prealign`; journal `align.run_prealign@1`), params measured, reference, seed (default 0, always recorded), k_candidates (1..=64, default 8), hints, candidate_index, sampling.voxel_size, parent. Create the alignment from AMBIG's candidate through the alignment element API (ALN.MODEL.ELEM) with status `ambiguous` when AMBIG says so; record seed, voxel size, K, method ID, hints digest and candidates. Any stage error: create and journal nothing. Tests: AC-1..AC-5. Constraints: CONSTITUTION §3, §8 (no `unwrap`/`expect`/`panic`/`unsafe`), D2 with a recorded seed.

## Open questions

- None.

## Children

_Leaf: none._
