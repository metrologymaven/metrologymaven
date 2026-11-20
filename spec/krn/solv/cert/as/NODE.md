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
built_pr: 316
satisfies: [REQ-KRN.SOLV.CERT-001, REQ-KRN.SOLV.CERT-004]
covers: []
depends_on: [KRN.SOLV.CONIC.WRAP, KRN.NUM.FN, KRN.NUM.ERR]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-20
---

# KRN.SOLV.CERT.AS: Active-set extraction, candidate enumeration and tie-break order

## Purpose

Reads a converged conic solution and proposes which constraints are in contact. Rows with small slack are the contact candidates; their multipliers split them into strongly active (clearly positive multiplier) and ambiguous (multiplier not clearly positive) for diagnosis only. Every set of the expected size drawn from the small-slack rows is listed in the documented tie-break order (lowest sorted point indices first), so the certificate pipeline tries them in a fixed order that does not depend on the solver's path, and the result is flagged `tie_broken` when there is more than one. The sets are an uncertified `Proposal`: only `certify` (`KRN.SOLV.CERT.CT`) turns one into a certified active set.

## Scope

**In:**
- `RowLabel { point: u32, side: u8 }` supplied by the caller for each constraint row (one point may own several rows)
- `Thresholds { slack_rel, mult_rel }` scaled by recorded data scales (max |b|, or 1 if that is 0, and max |y|), recorded in the output
- Exact classification of rows into strongly active, ambiguous and inactive (no rounding of the threshold products)
- Candidate enumeration: every set of the caller's expected size drawn from the small-slack rows (strongly active plus ambiguous), ordered lexicographically by sorted (point, side) labels, capped at a fixed maximum count; the same rule whether the strongly active rows number fewer than, exactly or more than the expected size
- `tie_broken` when more than one candidate exists; the diagnostic `over_determined` when more rows are strongly active than the expected size
- The `Proposal` type, whose sets are readable outside `fid-solv` only as uncertified sets
- Input checks: nonnegative cones only, lengths, finiteness, unique labels, valid thresholds and expected size

**Out** (and which node owns it instead):
- Solving on a candidate (`KRN.SOLV.CERT.RECOMP`) and checking optimality (`KRN.SOLV.CERT.KKT`)
- Trying candidates in order, packaging the certificate and turning a candidate into a certified active set (`KRN.SOLV.CERT.CT`)
- Producing the conic solution (`KRN.SOLV.CONIC.WRAP`)
- Rows of zero cones and second-order cones (see Open questions)

## Requirements

- **REQ-KRN.SOLV.CERT.AS-001** The module shall classify row i as strongly active when s_i <= slack_rel * scale_s and y_i >= mult_rel * scale_y and y_i > 0, as ambiguous when s_i <= slack_rel * scale_s and it is not strongly active, and as inactive otherwise, where scale_s and scale_y are recorded with the thresholds and both comparisons are exact in real arithmetic on the f64 inputs (the products are not rounded). A negative slack, however large, is a small slack.
- **REQ-KRN.SOLV.CERT.AS-002** When the small-slack rows (strongly active plus ambiguous) number exactly the expected size, the module shall return that single candidate with `tie_broken = false`.
- **REQ-KRN.SOLV.CERT.AS-003** When the small-slack rows number more than the expected size, the module shall return every subset of them of the expected size, up to 256 [unverified cap], sorted ascending by the lexicographic order of their sorted (point, side) labels, with `tie_broken = true`, and shall set `over_determined = true` exactly when the strongly active rows alone number more than the expected size. The candidate sets shall not depend on the multipliers beyond the classification of slacks, so a vertex solution and an interior solution of the same problem give the same list.
- **REQ-KRN.SOLV.CERT.AS-004** If fewer small-slack rows than the expected size exist, or the candidate count exceeds the cap, then `extract` shall return a typed `ActiveSetUndetermined { strongly_active, ambiguous, cause }` error with `cause` `TooFewRows` or `OverCap` respectively (the certificate pipeline's over-cap rule is REQ-007).
- **REQ-KRN.SOLV.CERT.AS-005** The output shall depend only on the problem's `b` and cones, the solution vectors `y` and `s`, the labels, the expected size and the thresholds, and shall be identical on x86-64, ARM64 and wasm32.
- **REQ-KRN.SOLV.CERT.AS-006** The candidate sets shall be returned as an uncertified `Proposal` that code outside `fid-solv` can read but not construct, and whose sets it can read only through an accessor named as uncertified.
- **REQ-KRN.SOLV.CERT.AS-007** When the certificate pipeline proposes sets and the candidate count exceeds the cap, the module shall, for a problem with `is_linear()` whose conic problem is an LP in exactly the expected number of variables, return one set: the basis of the exact phase-1 simplex with Bland's rule on the multiplier system `A_pool^T y = -q, y >= 0` over the small-slack rows in label order (`expected` independent rows with nonnegative multipliers, every sign decided exactly, independent of the solution's multipliers), or `ActiveSetUndetermined` with cause `NoBasis` when no such basis exists; and for any other problem it shall return the first 256 [unverified cap] sets of the lexicographic order, generated lazily, instead of `OverCap`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/solv/cert/as-v1` (40 solved minimum-zone LPs with known contact sets: 30 non-degenerate, 10 with planted ties such as two points at equal deviation), when extraction runs, then non-degenerate cases give exactly the known set with `tie_broken = false` and tied cases list candidates in lexicographic order, every planted basis among them, with `tie_broken = true`.
- **AC-2** Given a 5-point straightness problem whose points 1 and 3 are both exactly on the upper boundary and the expected size 3, then the candidates are [{0,1,3}, {0,1,4}, {0,3,4}, {1,3,4}] in that order (labels sorted by point), whatever the multipliers; the bases {0,1,4} and {0,3,4} appear in that order, so the lowest certifiable basis is {0,1,4}.
- **AC-3** (error path) Given a solution with only 2 rows of small slack and expected size 4, then `ActiveSetUndetermined` with cause `TooFewRows`.
- **AC-4** Given the same solution with rows presented in reversed order and labels permuted accordingly, then the same label sets and order are returned.
- **AC-5** Given a vertex solution and an interior solution of the same tied LP, then the candidate lists are identical.

## Design

- **Registry command(s):** none; library API `fid_solv::cert::active::{extract, RowLabel, Thresholds, Proposal, MAX_CANDIDATES}` and `fid_solv::cert::{CertError, Undetermined}`; crate-internal `propose(problem, solution, labels, expected, thresholds, linear)`, which `KRN.SOLV.CERT.CT`'s `certify` calls with `problem.is_linear()`.
- **Signature:** `extract(problem: &ConicProblem, solution: &ConicSolution, labels: &[RowLabel], expected: usize, thresholds: &Thresholds) -> Result<Proposal, CertError>`. The problem supplies `b` (for scale_s) and the cone layout, which the solution does not hold.
- **Data structures:** `Proposal { sets, strongly_active, ambiguous, thresholds, scales: (f64, f64), tie_broken, over_determined }`, plus the diagnostics `reduced` (the single set is the REQ-007 basis) and `truncated` (the sets are the first 256 of an over-cap enumeration), all fields `pub(crate)`, read through accessors (`uncertified_sets()`, `strongly_active()`, `ambiguous()`, `thresholds()`, `scales()`, `tie_broken()`, `over_determined()`, `reduced()`, `truncated()`). `strongly_active`, `ambiguous` and `over_determined` are diagnostics that depend on the multipliers and must not enter a certificate digest.
- **Algorithm:** check inputs; compute scales with total-order max of `abs`; classify each row with an exact comparison of `s_i` (and `y_i`) against the real product of threshold and scale, on integer significands and exponents; sort labels; check the count `C(small, expected)` against the cap exactly; enumerate the `expected`-combinations of the sorted small-slack rows in lexicographic order with an iterative combination generator. Over the cap (`propose`, REQ-007): for a linear LP in `expected` variables, the conic rows of the pool (label order) and `q` are converted exactly to integers (each column times its own power of two: odd significands, a positive scale changes no sign or ratio order), and a phase-1 simplex from the all-artificial start (rows with a negative right-hand side negated, artificial `j` at column `m + j`) runs on a fraction-free (Bareiss) integer tableau with exact division: entering = the lowest column (structural, then artificial) with a negative reduced cost, leaving = the least ratio, ties to the lowest basic index; phase-1 objective not 0 is `NoBasis`; zero-level artificials are driven out by the lowest nonbasic structural column with a nonzero entry, none is `NoBasis`. The big integers (sign and 32-bit limbs, Knuth algorithm D division) are local to the module. Otherwise the generator stops after 256 sets.
- **Errors:** `CertError` (non-exhaustive, shared with the sibling leaves): `Shape { field, expected, got }` for a length of `labels`, `y` or `s` other than the row count; `InvalidInput { field }` for thresholds, `expected = 0`, a non-nonnegative cone, or non-finite `y` or `s`; `DuplicateLabel { label }`; `ActiveSetUndetermined { strongly_active, ambiguous, cause: Undetermined }`, with `Undetermined` (non-exhaustive) `TooFewRows`, `OverCap` (`extract` only) or `NoBasis` (`propose`, REQ-007).
- **Determinism:** D0; comparisons only, total order, no float arithmetic beyond `abs`.
- **Performance budget:** 10 000 rows < 1 ms excluding enumeration; enumeration cap bounds the rest [unverified until measured].
- **Crate:** `fid-solv` (module `cert::active`).

## Test fixtures

`fixtures/solv/cert/as-v1`: generated by a committed seeded script with planted contact sets and ties; CC-BY-4.0. `fixtures/solv/cert/as-ties-v1`: over-cap tied pools of a straightness LP (200 and 13 tied rows, and a pool with no nonnegative basis) with the basis the REQ-007 rule gives, decided by an independent exact `Fraction` implementation of the same rule; CC-BY-4.0.

## Build prompt

> In crate `fid-solv`, add module `cert::active` with `extract(problem: &ConicProblem, sol: &ConicSolution, labels: &[RowLabel], expected: usize, th: &Thresholds) -> Result<Proposal, CertError>`. Scales: scale_s = max|b| (or 1 if zero), scale_y = max|y| (total-order max via `fid_num::f`). Classify rows exactly: strongly active (s <= slack_rel*scale_s and y >= mult_rel*scale_y and y > 0), ambiguous (small s, otherwise), inactive. Candidates: every set of size `expected` drawn from the small-slack rows, enumerated iteratively in lexicographic order of sorted (point, side) labels, capped at 256; `tie_broken` when more than one; `over_determined` when the strongly active rows exceed `expected`. Too few or over cap: `ActiveSetUndetermined` with its cause. For the certificate pipeline (`propose`, REQ-007), over the cap: a linear LP in `expected` variables gets one basis from an exact Bland-rule phase-1 simplex on `A_pool^T y = -q, y >= 0` (integer tableau, fraction-free pivoting; none is `NoBasis`); any other problem gets the first 256 sets, generated lazily. Record thresholds and scales. Return an uncertified `Proposal` (crate-private fields, accessors). Tests: `fixtures/solv/cert/as-v1`, `fixtures/solv/cert/as-ties-v1` (200 tied rows in bounded time), the 5-point tie example, the error case, row-permutation invariance, vertex versus interior solutions. Constraints (CONSTITUTION §4 KNP-1, §8): no FMA, total-order comparisons only, no HashMap/HashSet, no recursion, no `unsafe`, no I/O; deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`. Acceptance: AC-1..AC-5 of spec node KRN.SOLV.CERT.AS.

## Open questions

- Default threshold values (for example slack_rel 1e-9, mult_rel 1e-9 [unverified]) are set per caller; `KRN.MZ` and `KRN.ENV` should state theirs.
- **Follow-up, other cone types:** only nonnegative cones are classified today (`InvalidInput { field: "cones" }` otherwise). The consumer that needs more is `KRN.SOLV.SEQ`, whose SOCP subproblems (the minimum-zone cylinder among them) carry second-order cones; a second-order cone needs a per-cone activity rule (boundary of the cone, not per-row slack). Equality (zero-cone) rows are always active and could join every candidate as a fixed `base`, with `expected` counting only the inequality rows.
- **Amended at build (#316 review, 2026-11-20):** candidates are drawn from all small-slack rows in every case (path independence, REQ-003); `over_determined`, the `cause` of `ActiveSetUndetermined`, the `extract(problem, ..)` signature, the `y_i > 0` condition, exact comparison and the `Proposal` type are in the requirements and Design above. AC-2 now lists all four 3-subsets; AC-5 is new.
- **As built, fixture:** `fixtures/solv/cert/as-v1/make_cases.py` (stdlib Python, exact `Fraction` decisions, no libm) plants 40 minimum-zone straightness LPs (30 nondegenerate, 5 ties whose tied rows have sub-threshold multipliers, AC-2's problem among them, 5 ties with interior duals), 2 ties with vertex duals, 38 boundary cases (slack or multiplier at or within one ulp of the rounded threshold product, zero thresholds, negative values, exactly 252 candidates) and 3 undetermined cases. The oracle asserts that every LP case gives the same candidates whatever its multipliers; the test also solves all 42 LPs with `solve_lp` and checks the same lists. The crate's output bits are pinned by one SHA-256 for all targets (regression pin, not the oracle). The 256 cap and the performance budget are not measured [unverified].
- **TAN.PLANE AC-2 (pointer, from #344):** with REQ-007 the external-minimax crowned plate of `KRN.ENV.TAN.PLANE` (21 tied rows, `C(21, 4) = 5985` bases) certifies the AC's answer (one candidate {0}, 1.5 h, `rocking` false); `crates/fid-fit/tests/tan_plane.rs` now checks it. TAN.PLANE's own NODE.md still lists it as remaining and should be updated by its owner.
- **Judgement call:** the reduction applies only over the cap. Within the cap the full lexicographic list is kept, so `KRN.SOLV.CERT.CT`'s try order (its AC-2 and AC-3) and every existing output pin are unchanged.

## Change log

- REQ-004 amended and REQ-007 added 2026-11-20 (maintainer decision, #344 review): over the cap, the certificate pipeline reduces a linear problem's tied pool to one exact Bland-order basis and tries only the first 256 sets of any other problem; `extract` keeps `OverCap`.

## Children

_Leaf: none._
