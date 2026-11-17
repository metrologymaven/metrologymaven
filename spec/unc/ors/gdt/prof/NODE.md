---
id: UNC.ORS.GDT.PROF
title: Profile of a line and of a surface
level: 4
parent: UNC.ORS.GDT
kind: task
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.ORS.GDT-001, REQ-UNC.ORS.GDT-003]
covers: []
depends_on: [UNC.ORS.TRACE, UNC.ORS.GDT.GEOM]
uses_artifacts: [GDT.RES]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.ORS.GDT.PROF: Profile of a line and of a surface

## Purpose

Cases for `check.eval_line_profile@1` and `check.eval_surface_profile@1`: nominal analytic and freeform surfaces with constructed normal deviations, with and without datum references, bilateral and unequally disposed zones. The geometry and signed normal distances are closed-form, but how the zone is placed and whether the evaluation floats or is located depend on the callout, so the expected values are expert content.

## Scope

**In:**
- Constructed normal deviations on analytic nominal surfaces (projection closed-form, see `UNC.ORS.PROJ`)
- Datum-related and form-only profile, equal and unequal zone disposition

**Out:**
- Closed-form form, orientation and location cases (owned by `UNC.ORS.GDT.GEOM`); modifiers (owned by `UNC.ORS.GDT.MOD`); composite profile (owned by `UNC.ORS.GDT.COMP`)

## Requirements

- **REQ-UNC.ORS.GDT.PROF-001** Each profile DMT key marked `supported` shall have at least one case.
- **REQ-UNC.ORS.GDT.PROF-002** Every case shall state the expected deviation, the expected zone disposition and whether the zone is located by datums or floats.
- **REQ-UNC.ORS.GDT.PROF-003** Every case shall cite the clause and DMT row it exercises and state its dialect.

## Interfaces

- **Offers:** cases for suite `ors-gdt` in `ors-case/v1` format.
- **Consumes:** case format and checker (UNC.ORS.TRACE); generators (UNC.RDL.GEN); the pinned DMT as an artifact (GDT.RES).

## Decomposition hints

Content tasks per sub-topic once expert authors are assigned; agents may add geometry generators to `ors_gdt`.

## Open questions

- Interpretation content (expected results that depend on reading a standard, and every DMT row citation) is authored by paid or credentialed experts and signed off by two reviewers, per D-009; agents may draft structure, generators and closed-form mathematics only. Standards texts are provided under D-016.
- Profile cases depend on correct projection; failures should be cross-checked against `UNC.ORS.PROJ` results first.

## Children

_None yet: status `open`._
