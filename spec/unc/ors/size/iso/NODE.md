---
id: UNC.ORS.SIZE.ISO
title: ISO 14405-1 size modifier cases
level: 4
parent: UNC.ORS.SIZE
kind: task
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.ORS.SIZE-001, REQ-UNC.ORS.SIZE-002, REQ-UNC.ORS.SIZE-003]
covers: []
depends_on: [UNC.ORS.TRACE, UNC.ORS.SIZE.GEOM]
uses_artifacts: [GDT.RES]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.ORS.SIZE.ISO: ISO 14405-1 size modifier cases

## Purpose

Cases for the ISO 14405-1 size specification modifiers in the supported subset (two-point, least squares, minimum circumscribed, maximum inscribed, and rank-order modifiers if supported), on the geometry from `UNC.ORS.SIZE.GEOM`, with the same callout giving a different size than under ASME. Mapping modifiers to definitions is expert content; starts in P1b with the ISO GPS dialect.

## Scope

**In:**
- One case per supported modifier, chosen so its value differs from at least one other modifier
- Paired ASME and ISO cases on the same geometry

**Out:**
- Closed-form geometry (owned by `UNC.ORS.SIZE.GEOM`); ASME cases (owned by `UNC.ORS.SIZE.ASME`)

## Requirements

- **REQ-UNC.ORS.SIZE.ISO-001** Each ISO size modifier DMT key marked `supported` shall have at least one case whose expected value differs from another modifier on the same geometry.
- **REQ-UNC.ORS.SIZE.ISO-002** Every case shall state dialect `ISO_GPS`, the expected value and tolerance.
- **REQ-UNC.ORS.SIZE.ISO-003** Every case shall cite the clause and DMT row it exercises.

## Interfaces

- **Offers:** cases for suite `ors-size` in `ors-case/v1` format.
- **Consumes:** case format and checker (UNC.ORS.TRACE); geometry generators; the pinned DMT as an artifact (GDT.RES).

## Decomposition hints

One content task for an expert author; geometry comes from `ors_size::geom`.

## Open questions

- Interpretation content (expected results that depend on reading a standard, and every DMT row citation) is authored by paid or credentialed experts and signed off by two reviewers, per D-009; agents may draft structure, generators and closed-form mathematics only. Standards texts are provided under D-016.
- Rank-order and statistical size modifiers need closed-form geometry beyond lobed profiles [unverified: which are in the supported subset].

## Children

_None yet: status `open`._
