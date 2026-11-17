---
id: UNC.PROP.NLIN
title: Nonlinearity assessment for LPU
level: 3
parent: UNC.PROP
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.PROP-002]
covers: []
depends_on: [UNC.PROP.LPU.COMB, UNC.MODEL.GRAPH, UNC.UEC.STMT, KRN.MAN.KCI, KRN.MAN.CARD]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.PROP.NLIN: Nonlinearity assessment for LPU

## Purpose

Decides whether a first-order LPU result can be trusted for a given model and operating point, and records the outcome and its evidence values in the statement's `nonlinearity` field. The criterion `nlin@1` estimates the second-order variance contribution from deterministic evaluations of the model at sigma points of each input (plus or minus one standard uncertainty) and compares it with the first-order variance. This is a curvature probe on the model output, not a way to obtain sensitivities, which still come only from tangents. The spec card is written in our own words. It cites JCGM 100:2008 (the higher-order-terms note in its clause on uncorrelated input quantities) and the 2026 amendment on nonlinearity by designator and clause only [unverified: amendment clause numbers and criteria]. If the amendment's criteria differ, they become `nlin@2`.

## Scope

**In:**
- Spec card `nlin@1` (kernel spec-card registry), reviewed by an uncertainty expert before release
- Sigma-point curvature probe: for each input i, d_i = f(x + u_i e_i) + f(x - u_i e_i) - 2 f(x); mixed terms for the 32 inputs with the largest first-order contributions [unverified choice]
- Second-order variance estimate V2 = sum_i d_i^2 / 2 + sum over probed pairs of the mixed-term contribution
- Outcome `linear_adequate`, `second_order_needed` or `use_mcm`, with evidence values (V1, V2, ratio, probed input count, `cross_terms_truncated`)
- `lpu_reportable` flag on the outcome: true only for `linear_adequate`

**Out:**
- LPU itself (owned by `UNC.PROP.LPU`)
- Running MCM and refusing to commit an LPU result (owned by `UNC.PROP.SELECT` and `UNC.PROP.MCM`)
- The statement type (owned by `UNC.UEC.STMT`)

## Requirements

- **REQ-UNC.PROP.NLIN-001** When LPU is evaluated, the system shall run the nonlinearity assessment and record its outcome and evidence values in the statement's `nonlinearity` field.
- **REQ-UNC.PROP.NLIN-002** The outcome shall carry `lpu_reportable = false` for every outcome other than `linear_adequate`, so that `UNC.PROP.SELECT` does not commit the LPU result as the reported uncertainty without an MCM evaluation.
- **REQ-UNC.PROP.NLIN-003** The assessment criteria and their version shall be recorded as `nlin@<semver>` and cite JCGM 100 and its amendment by designator and clause only [unverified: clause numbers].
- **REQ-UNC.PROP.NLIN-004** The assessment shall return `use_mcm` whenever V1 is below 1e-6 times V2 (including V1 = 0), so the GUM cosine-error family (first derivative vanishing at the estimate) is never `linear_adequate`.
- **REQ-UNC.PROP.NLIN-005** The assessment shall return `linear_adequate` when V2 / V1 is at most 0.01, `second_order_needed` when it is at most 0.1, and `use_mcm` otherwise [unverified: thresholds pending expert review of the amendment].
- **REQ-UNC.PROP.NLIN-006** If any probe evaluation fails in the kernel, then the assessment shall return `use_mcm` with reason `probe_failed` and the input index, and shall not fail the propagation.

## Acceptance criteria

- **AC-1** Given `fixtures/unc/nlin/linear.json` (a linear gauge-block model), when assessed after LPU, then the outcome is `linear_adequate` with V2 = 0 exactly and `lpu_reportable = true`.
- **AC-2** Given `fixtures/unc/nlin/cosine_error.json` (length L cos(theta) at theta = 0 with u(theta) = 1 mrad), then the outcome is `use_mcm` and `lpu_reportable = false`.
- **AC-3** Given `fixtures/unc/nlin/product.json` (Y = X1 X2 with means 0 and unit uncertainties, a known second-order case), then V2 equals the analytic value to relative 1e-12 and the outcome is `use_mcm`.
- **AC-4** Given a model with 100 inputs, then evidence shows 32 probed pairs and `cross_terms_truncated = true`.
- **AC-5** Given a statement built from AC-1, then its `nonlinearity` field carries `nlin@1`, the outcome and V1, V2.
- **AC-6** Given AC-3 on x86-64, ARM64 and wasm32, then the outcome bytes are identical.

## Design

- **Registry command(s):** none; called by `UNC.PROP.SELECT` after `propagate_lpu`. Algorithm ID `unc.prop.assess_nonlinearity@1.0.0`, spec card `nlin@1`.
- **Data structures:** `NlinOutcome { version: "nlin@1", outcome, lpu_reportable, v1, v2, ratio, probed_inputs, probed_pairs, cross_terms_truncated, reason: Option<..> }`, written into `UncertaintyStatement.nonlinearity`.
- **Algorithm:** V1 from the LPU sensitivities and covariance; probes by kernel `evaluate` at x, x plus or minus u_i e_i (inputs in model order) and at the four corners for each probed pair; mixed term per pair from the corner evaluations; sums in fixed order. Correlated inputs are probed along their declared standard uncertainties only (flag `correlation_ignored_in_probe`).
- **Errors:** none surfaced; probe failures become `use_mcm` with reason.
- **Determinism:** D0.
- **Performance budget:** 2n + 4 * 496 + 1 kernel evaluations; under 2 s for a 1 000-point cylinder model [unverified until measured].
- **Crate:** `fid-kunc` (module `prop::nlin`; KNP-1 applies).

## Test fixtures

`fixtures/unc/nlin/`: hand-built models with analytic second-order values (linear, cosine error, product), CC-BY-4.0.

## Build prompt

> In crate `fid-kunc`, module `prop::nlin`, implement `assess_nonlinearity(model, x, lpu_result) -> NlinOutcome` (algorithm `unc.prop.assess_nonlinearity@1.0.0`, criteria `nlin@1`). Compute V1 from the LPU sensitivities and covariance. For each input i (model order), evaluate the model through the kernel `evaluate` at x plus or minus u_i e_i and form d_i = f+ + f- - 2 f0; add mixed terms from corner evaluations for the 32 inputs with largest first-order contributions (set `cross_terms_truncated` when inputs exceed 32). V2 = sum d_i^2 / 2 + mixed contributions, with fixed-order sums. Outcome: `use_mcm` if V1 < 1e-6 V2; else `linear_adequate` if V2/V1 <= 0.01, `second_order_needed` if <= 0.1, else `use_mcm`; `lpu_reportable` only for `linear_adequate`. A failed probe gives `use_mcm` with reason `probe_failed`. Write a spec card `nlin@1` in your own words citing JCGM 100:2008 and its nonlinearity amendment by designator and clause only (no pasted text). Tests AC-1..AC-6 with `fixtures/unc/nlin/`. Constraints: KNP-1, CONSTITUTION 8; sensitivities never come from these probes.

## Open questions

- **Applied:** the amendment text has still not been read for this spec. `nlin@1` uses the classical second-order-term criterion, which the free JCGM 100:2008 text describes. An uncertainty expert must compare it with the amendment and confirm or replace the thresholds as `nlin@2` [unverified].
- The 32-pair truncation bounds cost on point-cloud models; whether it misses relevant cross curvature is unmeasured [unverified].

## Children

_Leaf: none._
