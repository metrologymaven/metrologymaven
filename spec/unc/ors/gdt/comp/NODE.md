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

## Research hypotheses

D-036 rule 4. Children state these as `AC-H` criteria; pattern and simultaneity rules stay expert content (D-009).

- **H-1 (coverage)** 100 % of composite and simultaneous-requirement DMT keys marked `supported` have at least one case, measured by the REQ-GDT.RES-006 link check; falsified if any key lacks a case.
- **H-2 (discrimination)** The suite detects 100 % of three seeded structural faults (feature-relating segment evaluated with the pattern-locating segment's datum constraints, a simultaneous group evaluated as separate requirements, separate requirements merged into one group), measured by one `rdl.run_suite@1` per test-only mutant aim; falsified if any fault passes every case.
- **H-3 (dialect sensitivity)** For each simultaneous-requirement key whose default differs between ASME and ISO GPS [unverified: which keys], at least one paired case on the same geometry has a different expected outcome per dialect, measured by pairing cases by geometry digest; falsified if a dialect-sensitive key has no differing pair.
- **H-4 (constructed offsets)** The constructed per-feature offsets and pattern rotation agree with an independent 50-digit oracle within 1e-12 m and 1e-12 rad, measured by `UNC.RDL.GEN.XCHECK`; falsified on any disagreement.

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
