---
id: UNC.ORS.GDT
title: ORS-GDT: tolerance evaluation cases
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
depends_on: [UNC.ORS.TRACE, UNC.ORS.DRF, UNC.RDL.GEN]
uses_artifacts: [GDT.RES]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.ORS.GDT: ORS-GDT: tolerance evaluation cases

## Purpose

Cases for the GD&T characteristics in the supported subset: form, orientation, location (position with and without material modifiers and bonus), profile, runout, composite and simultaneous requirements. Each case has a closed-form or independently computed expected deviation, and for decision-bearing cases an expected outcome under a stated rule.

## Scope

**In:**
- One or more cases per characteristic type in the supported subset (P1a: the ASME Y14.5-2018 rows of `GDT.RES` REQ-GDT.RES-009)
- Position with MMC/LMC bonus and datum shift where supported
- Composite and simultaneous requirement cases

**Out:**
- Case format, citation checks and versioning (owned by `UNC.ORS.TRACE`)
- Running the suite (owned by `UNC.ORS.RUN`)
- The evaluation under test (owned by `GDT`, `ALN`, `SCAN` or `GEO` as applicable)

## Requirements

- **REQ-UNC.ORS.GDT-001** Each characteristic type marked `supported` in the pinned DMT shall have at least one case with a closed-form expected result.
- **REQ-UNC.ORS.GDT-002** Cases with material modifiers shall state the expected bonus tolerance and the expected deviation separately.
- **REQ-UNC.ORS.GDT-003** Every case shall link the DMT row it exercises, so that REQ-GDT.RES-006 (every supported row has an ORS case) can be checked.

## Research hypotheses

D-036 rule 4 (AGENTS.md rule 10). Children turn these into `AC-H` acceptance criteria. They concern structure, coverage and discrimination only; expected values that need interpretation stay expert work (D-009). Thresholds marked [unverified] are proposals, not results.

- **H-1 (coverage)** Every DMT key marked `supported` for the P1a ASME subset has at least one released `ors-gdt` case or a `gaps.json` entry, and the released case coverage of supported keys reaches 100 % [unverified: target] at the first `ors-gdt` release, measured by the REQ-GDT.RES-006 link check over the pinned DMT; falsified if any supported key has neither.
- **H-2 (closed-form agreement)** Every case with basis `closed_form_math` agrees with the independent `fiducial-oracles` result within its acceptance tolerance (≤ 1e-12 m for constructed form, orientation, location and runout geometry), measured by `UNC.RDL.GEN.XCHECK` on the released suite; falsified if any case disagrees.
- **H-3 (discrimination)** The released suite detects ≥ 90 % [unverified] of a published catalogue of seeded mathematical faults, each child contributing faults from its area (association substitution, zone shape, datum precedence, bonus omitted or doubled, datum shift ignored, segment confusion), measured by one `rdl.run_suite@1` per test-only mutant aim; falsified if detection is lower or the reference aim fails a case.
- **H-4 (public structure)** Every P1a characteristic type that occurs in the NIST MBE PMI validation and conformance test models [unverified: release] has at least one case, measured by tallying those public models' callout types against the suite; falsified if a supported type in those models has no case.
- **Gap:** no public reference set with expected GD&T evaluation results was found [unverified], so H-1 to H-3 are measured against this project's own published generators and oracle; public NMI data (NIST ATS, PTB softgauges, TraCIM) anchor only the least-squares form conventions.

## Interfaces

- **Offers:** suite `ors-gdt@<semver>` in `ors-case/v1` format.
- **Consumes:** case format and checker (UNC.ORS.TRACE); DMT rows (GDT.RES) where cases cite them; generators from `UNC.RDL.GEN` for constructed geometry.

## Decomposition hints

FORM, ORIENT, POS (position and modifiers), PROF (profile), RUN (runout), COMP (composite and simultaneous). Each sub-suite becomes a content task once its cases have expert authors.

## Open questions

- Split by kind of work, as agreed at review. GEOM generates constructed geometry whose deviation is closed-form mathematics under the operator chain of a proposed DMT key (minimum-zone form from equioscillation sets, orientation and location against form-perfect datums, runout of eccentric cylinders); its cases are drafts until an expert confirms each DMT row and two reviewers sign off (D-009). MOD, PROF and COMP need interpretation (bonus and shift rules, profile zone semantics, pattern and simultaneity rules) and stay open for expert authors. GDT.RES is read as an artifact (`uses_artifacts`, D-034).

- Interpretation content (expected results that depend on reading a standard, and every DMT row citation) is authored by paid or credentialed GD&T experts and signed off by two reviewers, per D-009; agents may draft structure, generators and closed-form mathematics only.
- Expected results that cannot be derived in closed form need an independent oracle result recorded as a cross-check [unverified: which cases].

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| UNC.ORS.GDT.GEOM | Closed-form form, orientation, location and runout drafts (leaf) | 001 003 | none |
| UNC.ORS.GDT.MOD | Position with material modifiers, bonus and datum shift | 001 002 003 | none |
| UNC.ORS.GDT.PROF | Profile of a line and of a surface | 001 003 | none |
| UNC.ORS.GDT.COMP | Composite and simultaneous requirements | 001 003 | none |
