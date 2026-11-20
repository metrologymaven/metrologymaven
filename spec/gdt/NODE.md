---
id: GDT
title: Dimensional and GD&T evaluation
level: 1
parent: FID
kind: domain
status: split
phase: P1a
size: XXL
owners: []
claimed_by:
satisfies: [REQ-FID-001, REQ-FID-008, REQ-FID-009]
covers: [GOM.DIM.*, GOM.GDT.*, PW.GDT.*, PCD.DIM.*, PCD.CAD.02, SA.GDT.*]
depends_on: [KRN, ALN, UNC, IO]
cli: [fid check *, fid gdt *]
mcp: [check_*, gdt_*]
journal: [check.*, gdt.*]
updated: 2026-11-19
---

# GDT: Dimensional and GD&T evaluation

## Purpose

Turns characteristics into results. It covers:
- size and dimensional checks;
- form, orientation, location, profile and runout tolerances;
- datum reference frames with mobility;
- material modifiers and bonus tolerance;
- composite tolerances.

Each characteristic is evaluated under an explicitly chosen ASME Y14.5-2018 or ISO GPS dialect, through a kernel-side **interpretation resolver** that turns a callout into an operator chain.

## Scope

**In:**
- Characteristic model (based on QIF)
- Dialect selection and defaults profiles
- Interpretation resolver and Dialect Mapping Table (DMT)
- Dimensions: distance, angle, diameter, radius, caliper, min/max, point deviations
- Form: flatness, straightness, roundness, cylindricity
- Orientation, location (position, including patterns), concentricity, coaxiality, symmetry
- Profile of a line and of a surface (with best-fit options)
- Circular and total runout
- Datum reference frames and datum mobility
- MMC/LMC and bonus tolerance
- Composite tolerances
- PMI to checks (auto-create characteristics from imported PMI)
- ResultRecord production

**Out:**
- Uncertainty and decision rules (UNC)
- Alignment solvers (ALN)
- Fits (KRN)
- Surface color maps (SCAN)
- Industry-specific analyses (IND)

## Requirements

- **REQ-GDT-001** Every characteristic shall name its dialect (ASME or ISO), standard edition and defaults profile, and evaluation shall refuse a characteristic with no stated dialect.
- **REQ-GDT-002** The interpretation resolver shall map each callout, its context and its defaults profile to a documented operator chain, and the chain's digest shall be recorded in the result.
- **REQ-GDT-003** The system shall evaluate the dimensional checks listed in Scope from points or features, and record the evaluation basis.
- **REQ-GDT-004** The system shall evaluate form, orientation, location, profile and runout tolerances, including patterns and composite frames, under both dialects as each callout is supported.
- **REQ-GDT-005** Datum reference frames shall be built from datum features with mobility, and each result shall record the transform used and whether it lies within the mobility envelope.
- **REQ-GDT-006** When PMI is imported, the system shall propose characteristics automatically, and a human shall confirm them before they count as the inspection plan.
- **REQ-GDT-007** Every evaluation shall emit a ResultRecord (CONSTITUTION §5), and the GDT domain shall never itself decide conformance without UNC's decision rule.
- **REQ-GDT-008** A published support matrix shall list every callout and modifier combination per dialect as supported, partial or unsupported, generated from the DMT.
- **REQ-GDT-009** Every evaluation result shall carry the ID, version and content digest of each interpretation rule (DMT row and defaults profile) it used, and if any of them lacks expert sign-off for its current content, then the result shall be labelled `interpretation: provisional` (D-045).
- **REQ-GDT-010** While any rule of the D-045 first-review subset is not signed off, no DMT row outside that subset shall be added or changed and no GDT leaf that serves only callouts outside it shall be claimed or marked built (D-045 pacing).

## Interfaces

- **Offers:** `check.*` and `gdt.*` commands; the characteristic model; the interpretation resolver; DMT rows; ResultRecords.
- **Consumes:** KRN fits, ALN frames, UNC decisions, IO (PMI import).

## Decomposition hints

CHAR (characteristic model, dialect and defaults), RES (interpretation resolver + DMT), DIM (dimensional checks), FORM, ORIENT, LOC (position, patterns, coaxiality, symmetry), PROF (profile), RUN (runout), DRF (datums and mobility), PMI (PMI to checks), MATRIX (support matrix).

## Open questions

- **D-045 (accepted 2026-11-19):** results name every interpretation rule they used and carry `interpretation: provisional` until those rules are signed off; GDT work beyond the first-review subset waits for that subset's sign-off. Added REQ-GDT-009 (owned by `GDT.CHAR.INTERP` and `GDT.RES.STATUS`) and REQ-GDT-010 (owned by `GDT.RES.PACE`).
- Standards interpretation needs a credentialed expert. The baseline budgets 150–300 h for the first subset. Who authors DMT rows in an open project? Candidate decision for the steering group.

## Split rationale

The dividing principle is **one evaluation pipeline, split at its stages and then by tolerance family**: data and contract (CHAR), interpretation (RES), reference frame (DRF), shared zone solver (ZONE), then the families that feed it (DIM, FORM, LOC, PROF), plus the PMI intake workflow (PMI). The cross-cutting concerns that every family needs (dialect discipline, the ResultRecord contract, interpretation, DRFs, modifiers and composite tiers) each have exactly one owner, so tolerance-family children stay thin and testable against ORS cases.

The support matrix (hint MATRIX) is folded into RES because it is generated from the DMT. ResultRecord production is in CHAR, which also binds UNC's decision so no family can decide conformance itself.

Phases follow CONSTITUTION §7 and the scope ladder in concept doc 05 §2.5: CHAR, RES, DIM, DRF, ZONE, FORM and LOC are P1a (ASME rows only; mobility and ISO rows arrive in P1b inside those children); PROF (profile, runout) and PMI (CAD + PMI) are P1b.

Alternatives considered:
- **One child per hint (11 children: separate ORIENT, RUN and MATRIX).** Rejected: above the 9-child limit, and orientation shares derived-feature extraction with location while runout shares nominal-deviation machinery with profile.
- **Split by dialect (ASME vs ISO).** Rejected: the dialect is data in the DMT; splitting by it would duplicate solvers and break REQ-GDT-008's single support matrix.
- **Split by evaluation basis (points vs features).** Rejected: most checks need both, so ownership would overlap.
- **Modifiers and composite inside LOC.** Rejected: MMC and composite frames also apply to orientation and profile, so one shared engine (ZONE) avoids three copies.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| GDT.CHAR | Characteristic model, dialects and result records | 001 007 009 | GOM.GDT.17, PCD.DIM.02, PCD.DIM.05, PW.GDT.08 |
| GDT.RES | Interpretation resolver, DMT and support matrix | 001 002 008 009 010 | none |
| GDT.DIM | Dimensional and size checks | 003 007 | GOM.DIM.*, PW.GDT.01, PCD.DIM.01, SA.GDT.02 |
| GDT.DRF | Datum reference frames and datum mobility | 005 | GOM.GDT.14, PW.GDT.07, SA.GDT.03 |
| GDT.ZONE | Tolerance-zone engine, modifiers and composite frames | 004 005 007 | GOM.GDT.15, PCD.DIM.03, PCD.DIM.04, PW.GDT.09 |
| GDT.FORM | Form tolerances | 004 007 | GOM.GDT.01–04, PW.GDT.02 |
| GDT.LOC | Orientation and location tolerances | 004 007 | GOM.GDT.05–10, PW.GDT.03, PW.GDT.04 |
| GDT.PROF | Profile and runout tolerances | 004 007 | GOM.GDT.11–13, PW.GDT.05, PW.GDT.06, PCD.DIM.06 |
| GDT.PMI | PMI to characteristics | 006 | GOM.GDT.16, PCD.CAD.02, SA.GDT.01 |
