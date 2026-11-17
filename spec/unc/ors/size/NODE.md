---
id: UNC.ORS.SIZE
title: ORS-SIZE: size definition cases
level: 3
parent: UNC.ORS
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.ORS-001, REQ-UNC.ORS-002, REQ-UNC.ORS-003]
covers: []
depends_on: [UNC.ORS.TRACE, UNC.RDL.GEN]
uses_artifacts: [GDT.RES]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.ORS.SIZE: ORS-SIZE: size definition cases

## Purpose

Cases for size characteristics: the ASME envelope requirement and, from P1b, the ISO 14405-1 size specification modifiers (two-point, least-squares, minimum circumscribed, maximum inscribed and others in the supported subset), on constructed features with known form error so that the different size definitions give provably different answers.

## Scope

**In:**
- Constructed cylinders and slots with known form deviations
- Expected size per definition in the supported subset, per dialect

**Out:**
- Case format, citation checks and versioning (owned by `UNC.ORS.TRACE`)
- Running the suite (owned by `UNC.ORS.RUN`)
- The evaluation under test (owned by `GDT`, `ALN`, `SCAN` or `GEO` as applicable)

## Requirements

- **REQ-UNC.ORS.SIZE-001** Each size definition in the supported subset shall have at least one case where it gives a different expected value from at least one other definition on the same geometry.
- **REQ-UNC.ORS.SIZE-002** Every case shall state its dialect, since the same callout can mean different sizes under ASME and ISO GPS.
- **REQ-UNC.ORS.SIZE-003** Every case shall cite the clause and DMT row it exercises.

## Interfaces

- **Offers:** suite `ors-size@<semver>` in `ors-case/v1` format.
- **Consumes:** case format and checker (UNC.ORS.TRACE); DMT rows (GDT.RES) where cases cite them; generators from `UNC.RDL.GEN` for constructed geometry.

## Decomposition hints

ASME (envelope cases), ISO (ISO 14405-1 modifiers, P1b). Each sub-suite becomes a content task once its cases have expert authors.

## Open questions

- Split by kind of work. GEOM generates features whose size under each association (two-point, least squares, minimum circumscribed, maximum inscribed) is closed-form mathematics, for example odd-lobed profiles where every two-point size is equal while circumscribed and inscribed sizes differ. Mapping a callout to one of those definitions under a dialect is interpretation and stays with the open children ASME and ISO (ISO from P1b). GDT.RES is read as an artifact (D-034).

- Interpretation content (expected results that depend on reading a standard, and every DMT row citation) is authored by paid or credentialed GD&T experts and signed off by two reviewers, per D-009; agents may draft structure, generators and closed-form mathematics only.
- Expected results that cannot be derived in closed form need an independent oracle result recorded as a cross-check [unverified: which cases].

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| UNC.ORS.SIZE.GEOM | Lobed and tapered features with closed-form sizes per association (leaf) | 001 003 | none |
| UNC.ORS.SIZE.ASME | ASME envelope and size cases | 002 003 | none |
| UNC.ORS.SIZE.ISO | ISO 14405-1 size modifier cases | 001 002 003 | none |
