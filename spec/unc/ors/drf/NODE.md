---
id: UNC.ORS.DRF
title: ORS-DRF: datum reference frame cases
level: 3
parent: UNC.ORS
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.ORS-001, REQ-UNC.ORS-002, REQ-UNC.ORS-003]
covers: []
depends_on: [UNC.ORS.TRACE, GDT.RES]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# UNC.ORS.DRF: ORS-DRF: datum reference frame cases

## Purpose

Cases that check datum reference frame establishment: primary, secondary and tertiary datum features, degrees of freedom constrained at each step, datum feature simulators, and material boundary modifiers on datum references, with expected frames derived in closed form from constructed geometry. Nothing found during research offers NMI-grade reference data for DRFs [unverified], so this suite is a project differentiator.

## Scope

**In:**
- Planar, cylindrical and mixed datum schemes; constrained degrees of freedom per datum
- Datum features at MMB/LMB where the supported subset includes them
- Constructed point sets with known simulator contact and known frame

**Out:**
- Case format, citation checks and versioning (owned by `UNC.ORS.TRACE`)
- Running the suite (owned by `UNC.ORS.RUN`)
- The evaluation under test (owned by `GDT`, `ALN`, `SCAN` or `GEO` as applicable)

## Requirements

- **REQ-UNC.ORS.DRF-001** Each DRF scheme in the supported subset shall have at least one case whose expected frame (origin and axes) is derived in closed form from the constructed geometry.
- **REQ-UNC.ORS.DRF-002** Every case shall state the expected frame with a tolerance of at most 1e-9 m on origin and 1e-9 rad on axes unless the case documents why a looser tolerance is needed.
- **REQ-UNC.ORS.DRF-003** Every case shall cite the clause and DMT row of the DRF rule it exercises.

## Interfaces

- **Offers:** suite `ors-drf@<semver>` in `ors-case/v1` format.
- **Consumes:** case format and checker (UNC.ORS.TRACE); DMT rows (GDT.RES) where cases cite them; generators from `UNC.RDL.GEN` for constructed geometry.

## Decomposition hints

PLANAR (3-2-1 planar schemes), CYL (cylindrical datums), MOD (datum material modifiers). Each sub-suite becomes a content task once its cases have expert authors.

## Open questions

- Interpretation content (expected results that depend on reading a standard, and every DMT row citation) is authored by paid or credentialed GD&T experts and signed off by two reviewers, per D-009; agents may draft structure, generators and closed-form mathematics only.
- Expected results that cannot be derived in closed form need an independent oracle result recorded as a cross-check [unverified: which cases].

## Children

_None yet: status `open`._
