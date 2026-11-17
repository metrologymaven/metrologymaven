---
id: UNC.ORS.FLT
title: ORS-FLT: filter, resampling and outlier cases
level: 3
parent: UNC.ORS
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.ORS-002, REQ-UNC.ORS-003]
covers: []
depends_on: [UNC.ORS.TRACE, UNC.RDL.GEN]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.ORS.FLT: ORS-FLT: filter, resampling and outlier cases

## Purpose

Cases for scan-data filtering (ISO 16610 series filters in the supported subset), resampling and outlier rejection, built on signals whose filtered response is known in closed form (sinusoids of known wavelength, steps, isolated spikes).

## Scope

**In:**
- Profile and areal filter responses on analytic signals
- Resampling on known surfaces
- Outlier policies on planted outliers with known indices

**Out:**
- Case format, citation checks and versioning (owned by `UNC.ORS.TRACE`)
- Running the suite (owned by `UNC.ORS.RUN`)
- The evaluation under test (owned by `GDT`, `ALN`, `SCAN` or `GEO` as applicable)

## Requirements

- **REQ-UNC.ORS.FLT-001** Each filter in the supported subset shall have a case whose expected transmission at stated wavelengths is closed form.
- **REQ-UNC.ORS.FLT-002** Each outlier policy shall have a case whose expected excluded index set is stated exactly.
- **REQ-UNC.ORS.FLT-003** Every case shall cite the clause it exercises without reproducing the standard.

## Interfaces

- **Offers:** suite `ors-flt@<semver>` in `ors-case/v1` format.
- **Consumes:** case format and checker (UNC.ORS.TRACE); DMT rows (GDT.RES) where cases cite them; generators from `UNC.RDL.GEN` for constructed geometry.

## Decomposition hints

PROFILE, AREAL, RESAMPLE, OUTLIER. Each sub-suite becomes a content task once its cases have expert authors.

## Open questions

- Split by evaluation family. Outlier policies are project-defined (`KRN.OUT`, `SCAN.CLOUD.FILTER`) and their cases are closed-form by construction, so OUTLIER is a leaf. No node yet owns an ISO 16610 filter command or a resampling command (`KRN.OUT` names GDT as owner of profile filtration, but no GDT leaf defines it), so FILTER and RESAMPLE stay open until their aims exist; FILTER's expected transmissions also need a filtration expert's check against the standard (D-016).

- Interpretation content (expected results that depend on reading a standard, and every DMT row citation) is authored by paid or credentialed GD&T experts and signed off by two reviewers, per D-009; agents may draft structure, generators and closed-form mathematics only.
- Expected results that cannot be derived in closed form need an independent oracle result recorded as a cross-check [unverified: which cases].

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| UNC.ORS.FLT.OUTLIER | Planted-outlier cases with exact excluded index sets (leaf) | 002 003 | none |
| UNC.ORS.FLT.FILTER | Profile and areal filter transmission cases | 001 003 | none |
| UNC.ORS.FLT.RESAMPLE | Resampling cases on known surfaces | 003 | none |
