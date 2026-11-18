---
id: UNC.ORS.FLT.FILTER
title: Profile and areal filter transmission cases
level: 4
parent: UNC.ORS.FLT
kind: task
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.ORS.FLT-001, REQ-UNC.ORS.FLT-003]
covers: []
depends_on: [UNC.ORS.TRACE, UNC.RDL.GEN]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.ORS.FLT.FILTER: Profile and areal filter transmission cases

## Purpose

Cases for the ISO 16610 series filters in the supported subset: analytic input signals (sinusoids of chosen wavelength, steps, isolated spikes) whose filtered output follows from the filter's transmission characteristic, with end-effect handling stated. Two things block a leaf: no node yet defines the filter registry command the cases would name as aim, and the expected transmissions must be checked against the purchased standard by a filtration expert.

## Scope

**In:**
- Sinusoid amplitude transmission at stated wavelengths relative to the cutoff
- Step and spike responses for linear filters
- End-effect regions excluded or handled as the filter command defines

**Out:**
- Outlier cases (owned by `UNC.ORS.FLT.OUTLIER`); resampling (owned by `UNC.ORS.FLT.RESAMPLE`)

## Requirements

- **REQ-UNC.ORS.FLT.FILTER-001** Each filter in the supported subset shall have a case whose expected transmission at at least three stated wavelengths is closed form, with tolerance stated.
- **REQ-UNC.ORS.FLT.FILTER-002** Every case shall cite the designator, edition and clause of the filter it exercises without reproducing the standard.
- **REQ-UNC.ORS.FLT.FILTER-003** Every case shall state the end-effect handling assumed and exclude affected samples from comparison.

## Research hypotheses

Transmission formulas must be checked against the purchased standard by a filtration expert (D-016); the hypotheses below concern agreement with public reference data and discrimination, not the reading of the standard.

- **H-1 (agreement with public references)** For each filter in the supported subset, case expected outputs agree with the output of public reference software or softgauge data on the same input (NIST surface metrology reference software, PTB and NPL type F1 and F2 softgauges; versions pinned by `UNC.RDL.EXT` [unverified]) within 1 nm or 0.1 % of amplitude, whichever is larger [unverified], outside the end-effect region; falsified if any filter disagrees.
- **H-2 (discrimination)** Cases detect at least 95 % [unverified] of seeded single faults (cutoff misread as frequency, wrong weighting-function constant, missing end-effect exclusion, wrong sampling interval), measured by mutation runs through `UNC.ORS.RUN`; falsified if fewer.
- **H-3 (coverage)** Cases exist for 100 % of filters in the supported subset, each at three or more wavelengths spanning at least one decade around the cutoff; falsified if any is missing.

## Interfaces

- **Offers:** cases for suite `ors-flt` in `ors-case/v1` format.
- **Consumes:** case format and checker (UNC.ORS.TRACE); generators (UNC.RDL.GEN).

## Decomposition hints

PROFILE (profile Gaussian and other linear filters), AREAL (areal filters), once the filter command exists.

## Open questions

- Owner of the filter command: `KRN.OUT` says profile filtration for form evaluation is GDT's, but no GDT node defines it; GDT's owners need to place it (cross-domain issue).
- Transmission formulas must be checked against the purchased ISO 16610 parts by a filtration expert (D-016) [unverified].

## Children

_None yet: status `open`._
