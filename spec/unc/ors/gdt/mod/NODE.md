---
id: UNC.ORS.GDT.MOD
title: Position with material modifiers, bonus and datum shift
level: 4
parent: UNC.ORS.GDT
kind: task
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.ORS.GDT-001, REQ-UNC.ORS.GDT-002, REQ-UNC.ORS.GDT-003]
covers: []
depends_on: [UNC.ORS.TRACE, UNC.ORS.GDT.GEOM]
uses_artifacts: [GDT.RES]
cli: []
mcp: []
journal: []
updated: 2026-11-22
---

# UNC.ORS.GDT.MOD: Position with material modifiers, bonus and datum shift

## Purpose

Cases for position (and other characteristics where the supported subset allows) with MMC or LMC on the toleranced feature and MMB or LMB on datums: constructed features of size with known actual mating and minimum material sizes, so that bonus tolerance and datum shift have definite expected values under each dialect. The bonus and shift rules are interpretation content for expert authors.

## Scope

**In:**
- Features of size with constructed size and axis offset, at MMC and LMC
- Datum features of size referenced at MMB and LMB with constructed clearance
- Expected bonus tolerance, expected deviation and expected shift, stated separately

**Out:**
- Closed-form RFS cases (owned by `UNC.ORS.GDT.GEOM`); profile (owned by `UNC.ORS.GDT.PROF`); composite and simultaneous (owned by `UNC.ORS.GDT.COMP`)

## Requirements

- **REQ-UNC.ORS.GDT.MOD-001** Each material-modifier DMT key marked `supported` shall have at least one case.
- **REQ-UNC.ORS.GDT.MOD-002** Every case shall state the expected bonus tolerance, the expected datum shift (where applicable) and the expected deviation as separate values with tolerances.
- **REQ-UNC.ORS.GDT.MOD-003** Every case shall cite the clause and DMT row it exercises and state its dialect.

## Research hypotheses

D-036 rule 4. Children state these as `AC-H` criteria; expected bonus and shift values remain expert content (D-009), and the hypotheses below test only the case structure.

- **H-1 (coverage)** 100 % of material-modifier DMT keys marked `supported` in the pinned DMT have at least one case, measured by the REQ-GDT.RES-006 link check; falsified if any key lacks a case.
- **H-2 (separation and discrimination)** Every case's geometry makes the expected bonus, the expected datum shift (where applicable) and the expected deviation each differ from zero and from one another by at least 10 times the case tolerance [unverified: factor], so that the suite detects 100 % of three seeded faults (bonus omitted, datum shift omitted, bonus applied at RFS), measured by one `rdl.run_suite@1` per test-only mutant aim; falsified if any of the three passes every case.
- **H-3 (constructed sizes)** The constructed actual mating and actual minimum material sizes agree with an independent 50-digit oracle within 1e-12 m, measured by `UNC.RDL.GEN.XCHECK`; falsified on any disagreement.

## Interfaces

- **Offers:** cases for suite `ors-gdt` in `ors-case/v1` format.
- **Consumes:** case format and checker (UNC.ORS.TRACE); generators (UNC.RDL.GEN); the pinned DMT as an artifact (GDT.RES).

## Decomposition hints

Content tasks per sub-topic once expert authors are assigned; agents may add geometry generators to `ors_gdt`.

## Open questions

- Interpretation content (expected results that depend on reading a standard, and every DMT row citation) is authored by paid or credentialed experts and signed off by two reviewers, per D-009; agents may draft structure, generators and closed-form mathematics only. Standards texts are provided under D-016.
- Whether datum-shift cases need an envelope-type expected result in `ors-case/v1` is shared with `UNC.ORS.DRF.MOD` [unverified].

## Children

_None yet: status `open`._
