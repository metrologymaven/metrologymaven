---
id: GDT.FORM
title: Form tolerances
level: 2
parent: GDT
kind: subsystem
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT-004, REQ-GDT-007]
covers: [GOM.GDT.01, GOM.GDT.02, GOM.GDT.03, GOM.GDT.04, PW.GDT.02]
depends_on: [GDT.ZONE, GDT.RES, KRN]
cli: [fid check *-flatness, fid check *-straightness, fid check *-roundness, fid check *-cylindricity]
mcp: [check_*_flatness, check_*_straightness, check_*_roundness, check_*_cylindricity]
journal: [check.*_flatness, check.*_straightness, check.*_roundness, check.*_cylindricity]
updated: 2026-11-16
---

# GDT.FORM: Form tolerances

## Purpose

Evaluates the datum-free form tolerances: flatness, straightness (of surface line elements and of a derived median line), roundness (circularity) and cylindricity. They are the first rung of the scope ladder, need no DRF, and map most directly onto KRN minimum-zone fits, so they form a self-contained subsystem that can ship early and be validated against TraCIM Chebyshev data.

## Scope

**In:**
- Flatness, straightness (line elements and derived median line, with MMC on the derived case via `GDT.ZONE`), roundness / circularity, cylindricity
- Extraction of toleranced points per DMT chain (cross-sections for roundness, line elements for straightness)
- Optional filtration and resampling steps where the DMT chain names them (filters from KRN)
- Reference association: minimum zone (default where the DMT row says so) and least-squares reference as a declared conservative variant
- Form result payload: form value, peak and valley points, reference element

**Out** (and which node owns it instead):
- Minimum-zone and least-squares fits, filters (owned by `KRN`)
- Zone solve and MMC on derived features (owned by `GDT.ZONE`)
- Orientation tolerances (owned by `GDT.LOC`)
- Profile of a line or surface without datums (owned by `GDT.PROF`)

## Requirements

- **REQ-GDT.FORM-001** The system shall evaluate flatness, straightness, roundness and cylindricity from measured points and report the form value, the reference element and the indices of the extreme points.
- **REQ-GDT.FORM-002** The form value shall be computed against the reference association named by the resolved DMT row, and the association used (minimum zone or least squares) shall be recorded in the ResultRecord.
- **REQ-GDT.FORM-003** While KRN minimum zone is unavailable for a feature type, a least-squares-referenced form value shall be reported only with a `conservative_upper_bound` flag and never as the minimum-zone value.
- **REQ-GDT.FORM-004** If fewer points are supplied than the DMT row's minimum for the characteristic (for example 4 for flatness, 5 for cylindricity [unverified: minima to be set per row]), then the system shall reject the evaluation with a typed error.
- **REQ-GDT.FORM-005** Minimum-zone form values shall match the TraCIM Chebyshev reference results for plane, 2D circle and cylinder within the default MPEs stated by the test [unverified].

## Interfaces

- **Offers:** `check.eval_flatness@1`, `check.eval_straightness@1`, `check.eval_roundness@1`, `check.eval_cylindricity@1` (R1 scratch, W2 when committed).
- **Consumes:** fits and filters (KRN); zone engine (GDT.ZONE) for the derived-median-line MMC case; operator chain (GDT.RES); ResultRecord builder (GDT.CHAR).

## Decomposition hints

FLAT, STRAIGHT (line element and derived), ROUND (with cross-section extraction), CYL.

## Open questions

- None beyond the inherited DMT authorship question.

## Split rationale

The dividing principle is **one shared core plus one child per symbol**: the policy every form check shares (minimum points, minimum zone versus least squares with the conservative flag, payload) lives in FCORE so the symbol leaves stay thin; then flatness, straightness, roundness and cylindricity. Flatness, roundness and cylindricity are plain minimum-zone problems with explicit extraction parameters and are finished as leaves; straightness stays open because its derived-median-line form needs expert derivation rules and the MMC engine.

Alternatives considered:
- **Hint split FLAT / STRAIGHT / ROUND / CYL only.** Rejected: REQ-GDT.FORM-002..004 would be repeated in four places.
- **Split straightness into line-element and derived children here.** Deferred to STRAIGHT's own expansion; both share one command.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| GDT.FORM.FCORE | Form evaluation contract and reference-association policy (leaf) | 001 002 003 004 | PW.GDT.02 |
| GDT.FORM.FLAT | Flatness (leaf) | 001 005 | GOM.GDT.01, PW.GDT.02 |
| GDT.FORM.STRAIGHT | Straightness of line elements and of a derived median line | 001 005 | GOM.GDT.02, PW.GDT.02 |
| GDT.FORM.ROUND | Roundness (circularity) with cross-section extraction (leaf) | 001 005 | GOM.GDT.03, PW.GDT.02 |
| GDT.FORM.CYL | Cylindricity (leaf) | 001 005 | GOM.GDT.04, PW.GDT.02 |
