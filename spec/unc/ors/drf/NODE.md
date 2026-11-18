---
id: UNC.ORS.DRF
title: ORS-DRF: datum reference frame cases
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

## Research hypotheses

DRF content mixes closed-form geometry (GEOM) and expert interpretation (SIM, MOD). These hypotheses concern structure, coverage and discrimination only; no hypothesis asserts what a datum rule means (D-009).

- **H-1 (coverage of the supported subset)** ORS-DRF has at least one case for 100 % of DRF schemes and datum-order permutations whose DMT key exists in the pinned DMT, and `gaps.json` lists every other scheme, measured by joining the suite with the DMT key list; falsified if a keyed scheme has no case.
- **H-2 (coverage of public models)** The schemes in ORS-DRF cover at least 80 % [unverified] of the datum reference frames, classified only by datum feature type and precedence, that appear in the NIST MBE PMI validation and conformance test models (CTC and FTC STEP AP242 files; release to be pinned by digest [unverified]), measured by a person tabulating each model's DRFs by feature type (`human_only`); falsified if coverage is lower.
- **H-3 (discrimination of geometry faults)** Form-perfect cases detect at least 95 % [unverified] of seeded single faults in frame construction (wrong precedence, missing orthogonalisation order, axis treated as point, handedness flip), measured by mutation runs through `UNC.ORS.RUN`; falsified if fewer.
- **H-4 (discrimination beyond form-perfect)** At least 90 % [unverified] of SIM and MOD cases fail an implementation that substitutes least-squares datum features and ignores material boundary modifiers, measured by running those cases against that substitute; falsified if fewer fail, which would show the cases add little over GEOM.
- **Reference-set gap:** no public NMI data with reference DRFs was found [unverified]; the NIST PMI models supply callouts and geometry but no expected frames.

## Interfaces

- **Offers:** suite `ors-drf@<semver>` in `ors-case/v1` format.
- **Consumes:** case format and checker (UNC.ORS.TRACE); DMT rows (GDT.RES) where cases cite them; generators from `UNC.RDL.GEN` for constructed geometry.

## Decomposition hints

PLANAR (3-2-1 planar schemes), CYL (cylindrical datums), MOD (datum material modifiers). Each sub-suite becomes a content task once its cases have expert authors.

## Open questions

- Split by kind of work (instruction at review: agents write structure, closed-form geometry and placeholders, not interpretations). GEOM builds form-perfect datum features, where every reading of the DRF rules gives the same frame, so expected frames are pure mathematics; its cases still carry a proposed DMT key and need expert confirmation and two-expert sign-off (D-009) before release. SIM (imperfect datum features and simulator contact) and MOD (material boundary modifiers) are interpretation content and stay open for expert authors. GDT.RES is now read as an artifact (`uses_artifacts`, CONSTITUTION §2.1 and D-034), not a code dependency.

- Interpretation content (expected results that depend on reading a standard, and every DMT row citation) is authored by paid or credentialed GD&T experts and signed off by two reviewers, per D-009; agents may draft structure, generators and closed-form mathematics only.
- Expected results that cannot be derived in closed form need an independent oracle result recorded as a cross-check [unverified: which cases].

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| UNC.ORS.DRF.GEOM | Form-perfect DRF cases with closed-form frames (leaf) | 001 002 003 | none |
| UNC.ORS.DRF.SIM | Datum features with form error and simulator contact | 001 002 003 | none |
| UNC.ORS.DRF.MOD | Datum references at MMB and LMB | 001 002 003 | none |
