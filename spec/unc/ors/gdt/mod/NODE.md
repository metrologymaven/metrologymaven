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
updated: 2026-11-17
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
