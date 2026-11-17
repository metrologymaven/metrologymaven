---
id: UNC.ORS.SIZE.ASME
title: ASME envelope and size cases
level: 4
parent: UNC.ORS.SIZE
kind: task
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.ORS.SIZE-002, REQ-UNC.ORS.SIZE-003]
covers: []
depends_on: [UNC.ORS.TRACE, UNC.ORS.SIZE.GEOM]
uses_artifacts: [GDT.RES]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.ORS.SIZE.ASME: ASME envelope and size cases

## Purpose

Cases for size under ASME Y14.5-2018 (P1a dialect): the envelope requirement at MMC and the local size limits, on the geometry from `UNC.ORS.SIZE.GEOM`, with expected size values and expected envelope verdicts as the DMT rows define them. Which association each requirement uses is interpretation content for expert authors.

## Scope

**In:**
- Envelope at MMC on lobed and tapered features
- Local size limits on the same geometry
- Expected values and outcomes per DMT row

**Out:**
- Closed-form geometry (owned by `UNC.ORS.SIZE.GEOM`); ISO GPS cases (owned by `UNC.ORS.SIZE.ISO`)

## Requirements

- **REQ-UNC.ORS.SIZE.ASME-001** Each ASME size DMT key marked `supported` shall have at least one case built on `UNC.ORS.SIZE.GEOM` geometry.
- **REQ-UNC.ORS.SIZE.ASME-002** Every case shall state dialect `ASME`, the expected value or outcome and its tolerance.
- **REQ-UNC.ORS.SIZE.ASME-003** Every case shall cite the clause and DMT row it exercises.

## Interfaces

- **Offers:** cases for suite `ors-size` in `ors-case/v1` format.
- **Consumes:** case format and checker (UNC.ORS.TRACE); geometry generators; the pinned DMT as an artifact (GDT.RES).

## Decomposition hints

One content task for an expert author; geometry comes from `ors_size::geom`.

## Open questions

- Interpretation content (expected results that depend on reading a standard, and every DMT row citation) is authored by paid or credentialed experts and signed off by two reviewers, per D-009; agents may draft structure, generators and closed-form mathematics only. Standards texts are provided under D-016.


## Children

_None yet: status `open`._
