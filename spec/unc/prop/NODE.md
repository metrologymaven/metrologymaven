---
id: UNC.PROP
title: Propagation engine: LPU, Monte Carlo and multivariate
level: 2
parent: UNC
kind: subsystem
status: split
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-UNC-002]
covers: []
depends_on: [UNC.MODEL, UNC.UEC, KRN]
cli: [fid unc propagate-*, fid unc compare-methods]
mcp: [unc_propagate_*, unc_compare_methods]
journal: [unc.propagate_*]
updated: 2026-11-16
---

# UNC.PROP: Propagation engine

## Purpose

Evaluates an Uncertainty Model to produce the uncertainty of each output: first-order linear propagation (LPU, JCGM 100:2008 with its 2026 nonlinearity amendment [unverified]), adaptive Monte Carlo propagation of distributions (MCM, JCGM 101:2008) that re-runs the full kernel pipeline per trial, and multivariate propagation of feature parameter vectors (JCGM 102:2011). It owns the versioned method-selection rule and the estimator policy. It is separate from `UNC.MODEL` because the same model must be evaluable by more than one method and the methods must be comparable.

## Scope

**In:**
- LPU with sensitivity coefficients from forward-mode differentiation through kernel operators (implicit differentiation at optimizer solutions)
- Nonlinearity assessment for LPU
- Adaptive MCM with counter-based seeded RNG and fixed trial blocks
- Validation of LPU against MCM and the `lpu_validated_by_mc` flag
- Multivariate propagation with full covariance of feature parameter vectors
- Method selection rule (versioned)
- Estimator policy per quantity kind (reported value is f(x), not the MC mean; bias disclosure for non-negative quantities near zero)
- Contributor ranking and method comparison outputs

**Out** (and which node owns it instead):
- Model definition and contributor library (owned by `UNC.MODEL`)
- Spatial uncertainty fields and transform uncertainty (owned by `UNC.XFM`)
- Conformance probability and decisions (owned by `UNC.DEC`)
- Result field schema (owned by `UNC.UEC`)

## Requirements

- **REQ-UNC.PROP-001** When LPU is selected, the system shall compute sensitivity coefficients analytically through the kernel pipeline and not by finite differences.
- **REQ-UNC.PROP-002** When LPU is used, the system shall run a nonlinearity assessment and record its outcome in the uncertainty statement [unverified: criteria per JCGM 100 Amendment 1].
- **REQ-UNC.PROP-003** When MCM is selected, the system shall re-evaluate the full kernel pipeline for each trial and stop adaptively when the numerical tolerance on the coverage interval endpoints is met, with a configurable trial cap defaulting to 10^6 [unverified].
- **REQ-UNC.PROP-004** Given the same model digest, inputs and seed, MCM shall return a bit-identical trial count and coverage interval on the same platform (determinism class D2).
- **REQ-UNC.PROP-005** When both LPU and MCM are run on a model, the system shall compare them by the JCGM 101 validation procedure and set `lpu_validated_by_mc` accordingly.
- **REQ-UNC.PROP-006** The system shall propagate feature parameter vectors with their full covariance matrix into downstream characteristics, so that correlated datum and feature uncertainty is not treated as independent.
- **REQ-UNC.PROP-007** The method-selection rule shall choose MCM for non-smooth quantities (minimum zone, max/min-based values, position with bonus) and may choose LPU for smooth pipelines, and the rule version shall be recorded in each result.
- **REQ-UNC.PROP-008** The reported value shall be the kernel value at the measured inputs and never the Monte Carlo mean, and for non-negative quantities near zero the statement shall disclose or correct the bias by a declared method.
- **REQ-UNC.PROP-009** The system shall report, for every evaluated model, its contributors ranked by contribution to the output variance.

## Interfaces

- **Offers:** `unc.propagate_lpu@1` (R1), `unc.propagate_mcm@1` (R1), `unc.propagate_auto@1` (R1), `unc.compare_methods@1` (R1); committing a propagated statement to a result is W2 (journal `unc.propagate_*`); MC sample sets for `UNC.DEC` and `UNC.XFM`.
- **Consumes:** `UncertaintyModel` from `UNC.MODEL`; kernel operators, differentiation support and the seeded RNG from KRN.

## Decomposition hints

LPU, NLIN (nonlinearity check), MCM, MVAR (multivariate), SELECT (method selection and estimator policy), RANK (contributors and comparison).

## Open questions

- GUM-7 rework of Supplement 1 may change the adaptive procedure; absorb via a `gum_suite_profile` field [unverified].

## Split rationale

Split by **engine and policy**: each propagation method is its own engine (LPU, MCM, MVAR), the two checks that decide whether LPU can be trusted are separate pure functions (NLIN for the amendment's nonlinearity criteria, VALID for the Supplement 1 comparison), ranking is a pure function over engine outputs (RANK), and SELECT owns the versioned choice of method, the estimator policy and the only W2 commit path. Engines stay free of policy so that a policy change is a version bump in one leaf.

Alternatives considered:
- **One child per GUM document** (100, 101, 102). Rejected: validation of 100 by 101 and the estimator policy span documents.
- **Commit inside each engine.** Rejected: three W2 paths would each need the stale-input check; one commit command in SELECT keeps the journal shape uniform.
- **NLIN inside LPU.** Rejected: its criteria come from a newly published text that needs its own spec card and expert review.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| UNC.PROP.LPU | Linear propagation with analytic sensitivities | 001 | none |
| UNC.PROP.NLIN | Nonlinearity assessment for LPU | 002 | none |
| UNC.PROP.MCM | Adaptive Monte Carlo propagation through the kernel pipeline | 003 004 | none |
| UNC.PROP.VALID | Validation of LPU against Monte Carlo (leaf) | 005 | none |
| UNC.PROP.MVAR | Multivariate propagation of feature parameter vectors | 006 | none |
| UNC.PROP.SELECT | Method selection, estimator policy and statement commit (leaf) | 007 008 | none |
| UNC.PROP.RANK | Contributor ranking (leaf) | 009 | none |
