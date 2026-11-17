---
id: UNC.DEC.EVAL
title: Rule evaluation and conformance probability
level: 3
parent: UNC.DEC
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.DEC-002, REQ-UNC.DEC-003, REQ-UNC.DEC-006, REQ-UNC.DEC-007, REQ-UNC.DEC-010]
covers: []
depends_on: [UNC.DEC.TYPE, UNC.UEC.STMT, UNC.RDL.REP, KRN]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# UNC.DEC.EVAL: Rule evaluation and conformance probability

## Purpose

The kernel function that turns (value, uncertainty statement, tolerance, rule) into an outcome. It computes the conformance probability from Monte Carlo samples when available, or from a normal approximation only when that is permitted, compares values with decimal limits by exact rational comparison, and always reports U, the probability and the U/T capability warning, even under simple acceptance.

## Scope

**In:**
- Conformance-probability rules: p = P(within limits), CONFORMS if p ≥ p_min, NONCONFORMS if p ≤ 1 − p_min, otherwise INDETERMINATE
- Simple acceptance: CONFORMS if the value is within the limits (exact comparison), else NONCONFORMS
- Guard band: binary guarded acceptance, CONFORMS inside [LSL + w, USL − w] with w = r·U, else NONCONFORMS; empty acceptance zone flagged
- One- and two-sided tolerances
- NOT_DECIDABLE reasons: `u_missing` (rule needs U and class is UEC-0), `pdf_unavailable` (no samples and normal approximation not permitted), `label_only`
- Capability warning when U/T exceeds a threshold (default 0.25 [unverified])
- Declaring the outcome field's replay equivalence class as exact (consumed by `UNC.RDL.REP`)

**Out:**
- Specific and global risk (owned by `UNC.DEC.RISK`)
- Choosing the rule and the `rule_missing` case (owned by `UNC.DEC.DEFAULT`)

## Requirements

- **REQ-UNC.DEC.EVAL-001** The function shall return exactly one `Outcome` for every input, and shall return `NOT_DECIDABLE` only with a reason.
- **REQ-UNC.DEC.EVAL-002** If the rule kind needs uncertainty (conformance probability or guard band) and the statement is UEC-0, then the function shall return `NOT_DECIDABLE(u_missing)`.
- **REQ-UNC.DEC.EVAL-003** When Monte Carlo samples are referenced by the statement, the function shall compute p as the fraction of samples within the limits, each compared with the decimal limits by exact rational comparison (`kdec`).
- **REQ-UNC.DEC.EVAL-004** When no samples are available, the function shall use the normal approximation with mean y and standard uncertainty u only if `lpu_validated_by_mc` is true or the statement's selection rule records LPU as permitted, and otherwise shall return `NOT_DECIDABLE(pdf_unavailable)`.
- **REQ-UNC.DEC.EVAL-005** For a conformance-probability rule, the function shall return CONFORMS when p ≥ p_min, NONCONFORMS when p ≤ 1 − p_min, and INDETERMINATE otherwise.
- **REQ-UNC.DEC.EVAL-006** For simple acceptance and guard-band rules, the function shall still report U and p when the statement is UEC-1..3, and shall raise `capability_warning` when U divided by the tolerance width exceeds the configured threshold.
- **REQ-UNC.DEC.EVAL-007** The function shall never convert a decimal limit to f64 to decide an outcome.
- **REQ-UNC.DEC.EVAL-008** The algorithm spec card shall declare the outcome field with replay equivalence class `exact`, so that any outcome difference in replay classifies as `DIVERGENT`.
- **REQ-UNC.DEC.EVAL-009** If the rule kind is `label_only`, then the function shall return `NOT_DECIDABLE(label_only)` and never a kernel decision.

## Acceptance criteria

- **AC-1** Given `fixtures/unc/dec/eval/cases.csv` (about 80 rows: rule kind, limits as decimals, y, u or U, sample-file reference, flags, expected outcome, expected p to 1e-12 absolute, expected warning), when evaluated, then every outcome and warning matches and p is within tolerance.
- **AC-2** (boundary) Given y = 10.0500004 mm as the nearest f64 and USL = 10.050 mm under simple acceptance (row `simple_just_over`), then the outcome is NONCONFORMS.
- **AC-3** (UEC-0) Given a UEC-0 statement under `dr:iso14253-1:2017/default@1`, then the outcome is `NOT_DECIDABLE(u_missing)`; under `dr:simple-acceptance@1` it is decided and p is absent.
- **AC-4** (pdf) Given an MCM-only statement without samples and `lpu_validated_by_mc = false`, then the outcome is `NOT_DECIDABLE(pdf_unavailable)`.
- **AC-5** (guard band) Given r = 1 and U ≥ half the tolerance width, then the outcome is NONCONFORMS with flag `empty_acceptance_zone`.
- **AC-6** (replay) Given two recorded evaluations whose p differs by 1e-7 across p_min (CONFORMS versus INDETERMINATE), when classified by the REP-1 classifier from `UNC.RDL.REP`, then the outcome is `DIVERGENT`.
- **AC-7** (portability) Outputs of AC-1 are bit-identical on wasm32, x86-64 and ARM64.

## Design

- **Registry command(s):** none; kernel algorithm `dec.rule.evaluate@1.0.0`, exposed by `decide.evaluate_rule@1` in `UNC.DEC.DEFAULT`.
- **Data structures:** `EvalInput { value: f64, kind: QuantityKind, statement: UncertaintyStatement, samples: Option<SampleSet>, tolerance: Tolerance{ lsl: Option<Decimal>, usl: Option<Decimal> }, rule: DecisionRule, warn_ratio: Decimal }`; `DecisionRecord { rule_id, rule_version, outcome, p_conformance: Option<f64>, U: Option<f64>, flags }`.
- **Algorithm:** normal CDF via a complementary error function implemented from `fid-num` `exp` with a fixed rational approximation (accuracy better than 1e-15 relative [unverified until tested]); sample counting with `kdec`; U/T ratio compared exactly with the decimal threshold.
- **Errors:** `invalid_tolerance` (no limits, or LSL > USL), `unit_mismatch`.
- **Determinism:** D0.
- **Performance budget:** 10^6 samples counted in under 50 ms [unverified until measured].
- **Crate:** `fid-kdec`.

## Test fixtures

`fixtures/unc/dec/eval/`: `cases.csv` with expected p values computed at 50 digits by an mpmath script, sample files generated with `UNC.MODEL.DIST` (seed `0xDEC0001`), project-authored, CC-BY-4.0.

## Build prompt

> In crate `fid-kdec` (kernel; KNP-1; transcendentals only via `fid-num`; no panics, no `unwrap`), add `evaluate(input: EvalInput) -> Result<DecisionRecord, EvalError>`, algorithm ID `dec.rule.evaluate@1.0.0`.
> Rules: `label_only` -> `NOT_DECIDABLE(label_only)`. Kinds needing U on a UEC-0 statement -> `NOT_DECIDABLE(u_missing)`. Probability: from samples, the fraction within limits with each sample compared to the decimal limits by `fid-num` `kdec` exact rational comparison; without samples, normal approximation N(y, u) via an erfc built on `fid-num` `exp`, allowed only if `lpu_validated_by_mc` is true or the statement's selection rule permits LPU, else `NOT_DECIDABLE(pdf_unavailable)`. Conformance-probability kind: CONFORMS if p ≥ p_min, NONCONFORMS if p ≤ 1 − p_min, else INDETERMINATE. Simple acceptance: inside limits (exact) -> CONFORMS else NONCONFORMS. Guard band: CONFORMS inside [LSL + rU, USL − rU] (exact decimal arithmetic on limits) else NONCONFORMS; flag `empty_acceptance_zone`. Always report U and p when available; `capability_warning` when U/(USL − LSL) > threshold (default 0.25). Never convert a decimal limit to f64 to decide. Declare the outcome field `exact` in the spec card.
> Tests: all rows of `fixtures/unc/dec/eval/cases.csv`; `simple_just_over` boundary row is NONCONFORMS; UEC-0 cases; pdf_unavailable; empty acceptance zone; REP-1 classifier (crate `fid-rep`, dev-dependency) gives DIVERGENT for an outcome flip; bit-identical outputs on wasm32, x86-64, ARM64.

## Open questions

- Guard-band variants other than binary guarded acceptance (guarded rejection, three-zone) are left to future rule versions [unverified: common contractual usage].

## Children

_Leaf: none._
