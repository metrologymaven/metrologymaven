---
id: UNC
title: Uncertainty, decision rules and verification data
level: 1
parent: FID
kind: domain
status: split
phase: P0
size: XL
owners: []
claimed_by:
satisfies: [REQ-FID-003, REQ-FID-007, REQ-FID-009, REQ-FID-018]
covers: [SA.UNC.*]
depends_on: [KRN]
cli: [fid unc *, fid decide *, fid rdl *]
mcp: [unc_*, decide_*, rdl_*]
journal: [unc.*, decide.*, rdl.*]
updated: 2026-11-16
---

# UNC: Uncertainty, decision rules and verification data

## Purpose

Makes every result honest:
- task-specific measurement uncertainty (GUM linear propagation and Monte Carlo);
- instrument uncertainty models;
- uncertainty of transforms;
- typed conformance decision rules (ISO 14253-1, simple acceptance, guard bands);
- the open reference data library (RDL) and reference suites used to validate the kernel and GD&T.

## Scope

**In:**
- Uncertainty evidence classes UEC-0..3
- GUM/LPU and Monte Carlo methods
- Instrument uncertainty models (CMM MPE, tracker, arm, scanner)
- Uncertainty fields for networks
- Transform uncertainty
- Decision rules and outcomes
- The RDL: reference datasets, synthetic generators, TraCIM client
- Open reference suites for GD&T (ORS)
- Measurability and visibility planning

**Out:**
- Fits (KRN)
- Network adjustment (LVM, which consumes UNC models)
- Signing (TRUST)

## Requirements

- **REQ-UNC-001** Every ResultRecord shall carry an uncertainty evidence class. UEC-0 shall be displayed as "not evaluated" and never filled in with a fabricated value.
- **REQ-UNC-002** The system shall propagate uncertainty by linear propagation (LPU) and by Monte Carlo (MCM), and shall state the method, coverage factor or interval, and model ID in the result.
- **REQ-UNC-003** The system shall provide instrument uncertainty models for CMMs, trackers, arms and scanners as versioned, parameterised data, and shall use them to weight networks and transforms.
- **REQ-UNC-004** Conformance decisions shall be produced only by typed decision rules (`dr:*`), with outcome `CONFORMS | NONCONFORMS | INDETERMINATE | NOT_DECIDABLE`. ISO 14253-1 shall be the default under ISO GPS. ASME shall require an explicit rule, shown on reports.
- **REQ-UNC-005** The RDL shall be a versioned, open collection of reference datasets and synthetic generators, and each kernel release shall be validated against a frozen RDL release set.
- **REQ-UNC-006** The system shall provide open reference suites with closed-form expected results for the supported GD&T subset, runnable in CI.
- **REQ-UNC-007** The system shall estimate the measurability (visibility and expected uncertainty) of planned measurements before execution, for network and CMM planning.

## Interfaces

- **Offers:** `unc.*`, `decide.*` and `rdl.*` commands; uncertainty model data; the decision rule engine; RDL datasets.
- **Consumes:** KRN numerics and fits.

## Decomposition hints

UEC (evidence classes and the result fields), LPU, MCM, INST (instrument models), DEC (decision rules), RDL (datasets, generators, TraCIM client), ORS (GD&T reference suites), PLAN (measurability).

## Open questions

- Licence terms for redistributing NIST and PTB datasets are unverified (founder-decisions D gaps). Carried into `UNC.RDL`.
- `rdl.*` was added to `journal:` at split time: self-test and suite runs commit journaled run records, which `unc.*` and `decide.*` did not cover. Reviewers should confirm.
- The `RDL` child is tagged P0 (TraCIM client and frozen release sets are needed to validate kernel 1.0), earlier than this domain's P1a; `UNC.XFM` is P1b and `UNC.PLAN` is P2. Phase tags are subject to steering-group review.

## Split rationale

The split follows the **flow of an honest result**: the uncertainty statement and evidence class a result carries (UEC), the model it is computed from (MODEL), the engines that evaluate it (PROP), the instrument data that feed it (INST), the frame and spatial extensions (XFM), the decision made with it (DEC), and the verification data that prove the kernel right (RDL, ORS). Planning before measurement (PLAN) consumes the same models predictively. Each child owns one artifact type (statement, model, engine output, instrument model, transform covariance or field, rule, dataset, suite case, measurability report), which keeps ownership MECE.

Alternatives considered:
- **LPU and MCM as separate children** (as the decomposition hints suggested). Rejected: method selection, LPU validation by MC and the estimator policy span both, so they would share responsibility; PROP keeps them together and can split at level 3.
- **Measurement model inside PROP.** Rejected: the model is the reviewable, attestable object with its own versioning and adoption flow, and is evaluated by more than one method.
- **ORS inside RDL.** Rejected: suite content needs GD&T interpretation expertise and clause traceability, while RDL is format, generators and runners; ORS depends on RDL.
- **Transform uncertainty inside ALN or LVM.** Rejected: UNC's scope assigns it here, and both ALN and LVM consume it.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| UNC.UEC | Uncertainty evidence classes and result uncertainty statement | 001 002 | none |
| UNC.MODEL | Measurement models and contributor library | 002 | none |
| UNC.PROP | Propagation engine: LPU, Monte Carlo and multivariate | 002 | none |
| UNC.INST | Instrument uncertainty models | 003 | SA.UNC.01 |
| UNC.XFM | Transform uncertainty and spatial uncertainty fields | 002 003 | SA.UNC.02, SA.UNC.03 |
| UNC.DEC | Typed conformance decision rules | 004 | none |
| UNC.RDL | Reference data library and kernel validation | 005 | none |
| UNC.ORS | Open reference suites for GD&T and uncertainty | 002 006 | none |
| UNC.PLAN | Measurability and visibility planning | 007 | SA.UNC.04 |
