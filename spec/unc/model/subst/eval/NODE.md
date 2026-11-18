---
id: UNC.MODEL.SUBST.EVAL
title: Component formulas, bias treatment and combination
level: 4
parent: UNC.MODEL.SUBST
kind: task
status: open
phase: P1a
size: L
owners: []
claimed_by:
needs_expert: uncertainty
satisfies: [REQ-UNC.MODEL.SUBST-003, REQ-UNC.MODEL.SUBST-004]
covers: []
depends_on: [UNC.MODEL.SUBST.INPUT, KRN.NUM]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.MODEL.SUBST.EVAL: Component formulas, bias treatment and combination

## Purpose

The kernel computation of the substitution method: each uncertainty component (calibration of the workpiece, procedure repeatability, systematic error, workpiece and material variation, and any others the standard names), the systematic deviation between the mean of the repeated runs and the calibrated value and how it is treated, and the combined expanded uncertainty. The component list and bias treatment must be checked against the purchased standard by an uncertainty expert, so this node stays open.

## Scope

**In:**
- Component formulas and their names, the bias treatment options (corrected, or included in U), coverage factor rule
- Result type `SubstitutionResult { components: Vec<Component{name, value, source}>, bias, bias_treatment, u_c, k, U }`

**Out:**
- Input checks (owned by `UNC.MODEL.SUBST.INPUT`)
- Command and validation record (owned by `UNC.MODEL.SUBST.CMD`)

## Requirements

- **REQ-UNC.MODEL.SUBST.EVAL-001** The evaluation shall report each component separately with its value in SI units and its source, and the combined expanded uncertainty with k [unverified: component naming per ISO 15530-3:2011].
- **REQ-UNC.MODEL.SUBST.EVAL-002** The evaluation shall report the systematic deviation between the mean of the repeated runs and the calibrated value and state whether it was corrected or included in U.
- **REQ-UNC.MODEL.SUBST.EVAL-003** The evaluation shall be a pure kernel function with determinism class D0 and shall agree with a 50-digit reference within 1e-12 relative on the fixture set.

## Research hypotheses

Measurable hypotheses against public reference data (D-036 rule 4). Thresholds marked [unverified] are targets, not results. The children named in each item must state it as `AC-H` acceptance criteria.

- **H-1** The combined expanded uncertainty covers the known true value at the nominal 95 % rate within the exact binomial band of `UNC.ORS.UNC.HARNESS` on a public seeded calibrated-workpiece generator [gap: not yet defined in RDL or ORS-UNC].
- **H-2** Each bias treatment (corrected, or included in U) is tested separately on that generator with a known machine systematic error swept from 0 to 3 times the repeatability standard deviation [unverified]; the hypothesis is that both cover within the band, falsified for a treatment whose coverage leaves the band at any swept value.
- **H-3** The combination equals `UNC.PROP.LPU.COMB` applied to the same linear component model to relative 1e-12 [unverified], measured on the fixture set and the generator cases.

## Interfaces

- **Offers:** `evaluate(CheckedInput, options) -> SubstitutionResult` in `fid-kunc`.
- **Consumes:** `CheckedInput` (UNC.MODEL.SUBST.INPUT).

## Decomposition hints

One kernel leaf once an expert has fixed the component list and bias treatment.

## Open questions

- Component names, minimum run counts and the bias treatment must be checked against the purchased text of ISO 15530-3:2011 and any newer edition [unverified]. Needs an uncertainty expert.

## Children

_None yet: status `open`._
