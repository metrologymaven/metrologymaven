---
id: UNC.ORS.GDT.COMP
title: Composite and simultaneous requirements
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

# UNC.ORS.GDT.COMP: Composite and simultaneous requirements

## Purpose

Cases for composite position and composite profile (pattern-locating and feature-relating segments) and for simultaneous requirements across patterns sharing a datum reference frame. Expected results depend on pattern rules that are interpretation content for expert authors; agents may generate hole patterns with known per-hole offsets.

## Scope

**In:**
- Hole patterns with constructed per-feature offsets and pattern rotation
- Composite segments and simultaneous-requirement groupings with expected per-segment results

**Out:**
- Single-segment characteristics (owned by `UNC.ORS.GDT.GEOM`, `UNC.ORS.GDT.MOD`, `UNC.ORS.GDT.PROF`)

## Requirements

- **REQ-UNC.ORS.GDT.COMP-001** Each composite and simultaneous-requirement DMT key marked `supported` shall have at least one case.
- **REQ-UNC.ORS.GDT.COMP-002** Every case shall state the expected result per segment or per group with tolerances.
- **REQ-UNC.ORS.GDT.COMP-003** Every case shall cite the clause and DMT row it exercises and state its dialect.

## Interfaces

- **Offers:** cases for suite `ors-gdt` in `ors-case/v1` format.
- **Consumes:** case format and checker (UNC.ORS.TRACE); generators (UNC.RDL.GEN); the pinned DMT as an artifact (GDT.RES).

## Decomposition hints

Content tasks per sub-topic once expert authors are assigned; agents may add geometry generators to `ors_gdt`.

## Open questions

- Interpretation content (expected results that depend on reading a standard, and every DMT row citation) is authored by paid or credentialed experts and signed off by two reviewers, per D-009; agents may draft structure, generators and closed-form mathematics only. Standards texts are provided under D-016.
- Simultaneous requirements differ between ASME and ISO GPS defaults [unverified]; cases must be dialect-specific.

## Children

_None yet: status `open`._
