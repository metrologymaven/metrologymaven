---
id: KRN.SOLV.CERT.AS
title: Active-set extraction, candidate enumeration and tie-break order
level: 4
parent: KRN.SOLV.CERT
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-solv]
satisfies: [REQ-KRN.SOLV.CERT-001, REQ-KRN.SOLV.CERT-004]
covers: []
depends_on: [KRN.SOLV.CONIC.WRAP, KRN.NUM.FN, KRN.NUM.ERR]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.SOLV.CERT.AS: Active-set extraction, candidate enumeration and tie-break order

## Purpose

Reads a converged conic solution and decides which constraints are in contact. Rows that are clearly active (small slack, clearly positive multiplier) are kept; rows that are ambiguous (small slack, near-zero multiplier) make the set degenerate. When more than one set of the expected size is consistent with the thresholds, this leaf lists the candidates in the documented tie-break order (lowest sorted point indices first), so the certificate pipeline tries them in a fixed order and flags the result `tie_broken`.

## Scope

**In:**
- `RowLabel { point: u32, side: u8 }` supplied by the caller for each constraint row (one point may own several rows)
- `Thresholds { slack_rel, mult_rel }` scaled by recorded data scales (max |b| and max |y|), recorded in the output
- Classification of rows into strongly active, ambiguous and inactive
- Candidate enumeration: all sets of the caller's expected size that contain every strongly active row and are drawn from strongly active plus ambiguous rows, ordered lexicographically by sorted (point, side) labels, capped at a fixed maximum count
- `tie_broken` when more than one candidate exists

**Out** (and which node owns it instead):
- Solving on a candidate (`KRN.SOLV.CERT.RECOMP`) and checking optimality (`KRN.SOLV.CERT.KKT`)
- Trying candidates in order and packaging the certificate (`KRN.SOLV.CERT.CT`)
- Producing the conic solution (`KRN.SOLV.CONIC.WRAP`)

## Requirements

- **REQ-KRN.SOLV.CERT.AS-001** The module shall classify row i as strongly active when s_i <= slack_rel * scale_s and y_i >= mult_rel * scale_y, as ambiguous when s_i <= slack_rel * scale_s and y_i < mult_rel * scale_y, and as inactive otherwise, where scale_s and scale_y are recorded with the thresholds.
- **REQ-KRN.SOLV.CERT.AS-002** When the strongly active rows number exactly the expected size and there are no ambiguous rows, the module shall return that single candidate with `tie_broken = false`.
- **REQ-KRN.SOLV.CERT.AS-003** When more than one candidate of the expected size exists, the module shall return all of them, up to 256 [unverified cap], sorted ascending by the lexicographic order of their sorted (point, side) labels, with `tie_broken = true`.
- **REQ-KRN.SOLV.CERT.AS-004** If no candidate of the expected size exists, or the candidate count exceeds the cap, then the module shall return a typed `ActiveSetUndetermined { strongly_active, ambiguous }` error.
- **REQ-KRN.SOLV.CERT.AS-005** The output shall depend only on the solution vectors, the labels, the expected size and the thresholds, and shall be identical on x86-64, ARM64 and wasm32.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/solv/cert/as-v1` (40 solved minimum-zone LPs with known contact sets: 30 non-degenerate, 10 with planted ties such as two points at equal deviation), when extraction runs, then non-degenerate cases give exactly the known set with `tie_broken = false` and tied cases list candidates in lexicographic order with `tie_broken = true`.
- **AC-2** Given a 5-point straightness problem whose points 1 and 3 are both exactly on the upper boundary and the expected size 3, then the candidates are [{0,1,4}, {0,3,4}] in that order (labels sorted by point), and the first is {0,1,4}.
- **AC-3** (error path) Given a solution with only 2 rows of small slack and expected size 4, then `ActiveSetUndetermined`.
- **AC-4** Given the same solution with rows presented in reversed order and labels permuted accordingly, then the same label sets and order are returned.

## Design

- **Registry command(s):** none; library API `fid_solv::cert::active::{extract, RowLabel, Thresholds, Candidates}`.
- **Data structures:** `Candidates { sets: Vec<Vec<RowLabel>>, strongly_active: Vec<RowLabel>, ambiguous: Vec<RowLabel>, thresholds: Thresholds, scales: (f64, f64), tie_broken: bool }`.
- **Algorithm:** compute scales with total-order max; classify; sort labels; enumerate k-combinations of the ambiguous rows (k = expected size minus strongly active count) in lexicographic order with an iterative combination generator, stopping at the cap.
- **Errors:** `CertError::ActiveSetUndetermined { strongly_active, ambiguous }`, `CertError::Shape` for label count mismatch.
- **Determinism:** D0; comparisons only, total order.
- **Performance budget:** 10 000 rows < 1 ms excluding enumeration; enumeration cap bounds the rest [unverified until measured].
- **Crate:** `fid-solv` (module `cert::active`).

## Test fixtures

`fixtures/solv/cert/as-v1`: generated by a committed seeded script with planted contact sets and ties; CC-BY-4.0.

## Build prompt

> In crate `fid-solv`, add module `cert::active` with `extract(sol: &ConicSolution, labels: &[RowLabel], expected: usize, th: &Thresholds) -> Result<Candidates, CertError>`. Scales: scale_s = max|b| (or 1 if zero), scale_y = max|y| (total-order max via `fid_num::fn`). Classify rows: strongly active (s <= slack_rel*scale_s and y >= mult_rel*scale_y), ambiguous (small s, small y), inactive. Candidates: every set of size `expected` containing all strongly active rows plus ambiguous rows, enumerated iteratively in lexicographic order of sorted (point, side) labels, capped at 256; `tie_broken` when more than one. None or over cap: `ActiveSetUndetermined`. Record thresholds and scales in the output. Tests: `fixtures/solv/cert/as-v1`, the 5-point tie example, the error case, row-permutation invariance. Constraints (CONSTITUTION §4 KNP-1, §8): no FMA, total-order comparisons only, no HashMap/HashSet, no recursion, no `unsafe`, no I/O; deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`. Acceptance: AC-1..AC-4 of spec node KRN.SOLV.CERT.AS.

## Open questions

- Default threshold values (for example slack_rel 1e-9, mult_rel 1e-9 [unverified]) are set per caller; `KRN.MZ` and `KRN.ENV` should state theirs.
- **As built (crates/fid-solv, module `cert::active`):** the signature is `extract(problem: &ConicProblem, solution: &ConicSolution, labels, expected, thresholds)`. The problem is needed because `scale_s = max |b|` and the cone layout are not in the solution. Every cone must be nonnegative (`InvalidInput { field: "cones" }` otherwise): a slack/multiplier rule does not classify equality rows or second-order cones. Thresholds must be finite and nonnegative, `expected >= 1`, `y` and `s` finite (the solution's fields are public), labels unique (`DuplicateLabel`).
- **As built, exactness:** both threshold tests compare `s_i` with the real product `slack_rel * scale_s` (and `y_i` with `mult_rel * scale_y`) exactly, on integer significands and exponents; the product is never rounded. There is no rounding band, so no extra tolerance is needed. The only tolerances are the caller's two thresholds, recorded with the scales. The only float operations are `abs` and total-order max, so the output is bit-identical on every target.
- **As built, strongly active also needs `y_i > 0`:** with `mult_rel = 0` or all multipliers 0, a zero multiplier is not clearly positive, so the row is ambiguous (fail closed).
- **As built, over-determined sets (deviation from REQ-004):** when more rows are strongly active than `expected` (a primal-degenerate optimum such as two points at equal deviation, where an interior-point method gives every tied row a positive multiplier), the candidates are the `expected`-size subsets of the strongly active rows, in the same order, with `tie_broken = true` and a new flag `over_determined = true`. REQ-004 as written would return `ActiveSetUndetermined` here, so a solved tie would never reach `KRN.SOLV.CERT.CT`; candidates are only proposals, and `CT` certifies one by recomputation and the KKT check or fails. Over the cap is still `ActiveSetUndetermined`.
- **As built, error:** `CertError` (in `fid_solv::cert`, non-exhaustive for the sibling leaves) has `Shape { field, expected, got }`, `InvalidInput { field }`, `DuplicateLabel { label }` and `ActiveSetUndetermined { strongly_active, ambiguous, cause }`, where `cause` is `TooFewRows` or `OverCap`.
- **As built, fixture:** `fixtures/solv/cert/as-v1/make_cases.py` (stdlib Python, exact `Fraction` decisions, no libm) plants 40 minimum-zone straightness LPs (30 nondegenerate, 5 ties whose tied rows have sub-threshold multipliers, AC-2's problem among them, 5 over-determined ties), 2 vertex-dual ties, 38 boundary cases (slack or multiplier at or within one ulp of the rounded threshold product, zero thresholds, negative values, exactly 252 candidates) and 3 undetermined cases. The test also solves all 42 LPs with `solve_lp` and checks the candidates the oracle predicts for an interior-point solution. The crate's output bits are pinned by one SHA-256 for all targets (regression pin, not the oracle). The 256 cap and the performance budget are not measured [unverified].

## Children

_Leaf: none._
