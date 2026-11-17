---
id: UNC.ORS.ALN
title: ORS-ALN: alignment cases
level: 3
parent: UNC.ORS
kind: module
status: split
phase: P1a
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

# UNC.ORS.ALN: ORS-ALN: alignment cases

## Purpose

Cases for alignments that are not GD&T characteristics but feed them: 3-2-1, RPS, best-fit and constrained best-fit, with constructed point sets whose optimal transform is known by construction. ORS-ALN cases are also the reference for transform covariance agreement checks in `UNC.XFM`.

## Scope

**In:**
- 3-2-1 and RPS with exact solutions
- Best-fit with constructed residuals orthogonal to the solution's Jacobian (null-space construction)
- Constrained best-fit with known active constraints

**Out:**
- Case format, citation checks and versioning (owned by `UNC.ORS.TRACE`)
- Running the suite (owned by `UNC.ORS.RUN`)
- The evaluation under test (owned by `GDT`, `ALN`, `SCAN` or `GEO` as applicable)

## Requirements

- **REQ-UNC.ORS.ALN-001** Each alignment method in ALN's P1a scope shall have at least one case whose expected transform is known by construction.
- **REQ-UNC.ORS.ALN-002** Every case shall state the expected transform in a declared parameterisation with tolerances on translation (m) and rotation (rad).
- **REQ-UNC.ORS.ALN-003** Cases used for `UNC.XFM` agreement checks shall be tagged `xfm_reference`.

## Interfaces

- **Offers:** suite `ors-aln@<semver>` in `ors-case/v1` format.
- **Consumes:** case format and checker (UNC.ORS.TRACE); DMT rows (GDT.RES) where cases cite them; generators from `UNC.RDL.GEN` for constructed geometry.

## Decomposition hints

P321, RPS, BEST, CONSTR. Each sub-suite becomes a content task once its cases have expert authors.

## Open questions

- Split by construction method. Alignment cases rest on mathematics, not on reading a GD&T standard, so all three children are leaves whose cases carry `citations: none` with basis `closed_form_math`; none cites a DMT row. Datum-system alignment (`align.run_datum_system@1`) is a DRF interpretation and belongs to `UNC.ORS.DRF`. ORS is Tooling (CONSTITUTION §2.1): cases name their aim by registry command ID and `UNC.ORS.RUN` dispatches them; no product crate is linked.

- Interpretation content (expected results that depend on reading a standard, and every DMT row citation) is authored by paid or credentialed GD&T experts and signed off by two reviewers, per D-009; agents may draft structure, generators and closed-form mathematics only.
- Expected results that cannot be derived in closed form need an independent oracle result recorded as a cross-check [unverified: which cases].

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| UNC.ORS.ALN.EXACT | Exact-solution cases for 3-2-1, RPS and point-pair alignments (leaf) | 001 002 003 | none |
| UNC.ORS.ALN.BEST | Best-fit cases with null-space constructed residuals (leaf) | 001 002 003 | none |
| UNC.ORS.ALN.CONSTR | Constrained best-fit cases with known active constraints (leaf) | 001 002 | none |
