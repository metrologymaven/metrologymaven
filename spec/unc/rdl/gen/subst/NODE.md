---
id: UNC.RDL.GEN.SUBST
title: Substitution-method workpiece generator
level: 4
parent: UNC.RDL.GEN
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.RDL.GEN-006]
covers: []
depends_on: [UNC.RDL.FORMAT, UNC.RDL.GEN.HPREC]
uses_artifacts: [UNC.MODEL.SUBST]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.RDL.GEN.SUBST: Substitution-method workpiece generator

## Purpose

No public calibrated-workpiece data set exists for validating substitution-method uncertainty. This generator produces synthetic calibrated workpieces with known reference values and known measurement-process errors, including deliberate strategy mismatches, so UNC.MODEL.SUBST hypotheses become testable on open data.

## Scope

**In:**
- Workpiece families with known reference values and calibration uncertainty
- Simulated repeated measurements with known systematic and random components
- Heterogeneous and mismatched-strategy cases (`conditions_mismatch`)
- Seeded, reproducible output in RDL format

**Out:**
- Interpretation of standards (expert work, D-009); this node supplies data and structure only

## Requirements

- **REQ-UNC.RDL.GEN.SUBST-001** The generator shall produce calibrated-workpiece cases whose reference values and error components are known exactly from the seed.
- **REQ-UNC.RDL.GEN.SUBST-002** The generator shall include cases where the measuring strategy differs from the calibration strategy, flagged as expected mismatches.

## Research hypotheses

- **H-1** Substitution-method uncertainty intervals computed on generated cases cover the known reference value within the exact 99 % binomial band around the stated coverage over N ≥ 1000 [unverified] cases, and every seeded mismatch is flagged.

## Interfaces

To be refined when this node is expanded.

## Decomposition hints

Split along the Scope items above.

## Open questions

- Added from the research-hypothesis gap list (generated/reviews/RESEARCH-HYPOTHESES.md), 2026-11-17. Thresholds are targets marked [unverified].

## Children

_None yet: status `open`._
