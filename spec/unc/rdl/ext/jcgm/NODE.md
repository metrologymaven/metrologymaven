---
id: UNC.RDL.EXT.JCGM
title: JCGM worked-example fixtures
level: 4
parent: UNC.RDL.EXT
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.RDL.EXT-006]
covers: []
depends_on: [UNC.RDL.EXT.IMP, UNC.RDL.EXT.LIC]
uses_artifacts: [UNC.PROP, UNC.ORS.UNC]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.RDL.EXT.JCGM: JCGM worked-example fixtures

## Purpose

The JCGM guides are freely available and contain worked examples with published results, the only public reference values found for uncertainty propagation. This node encodes their inputs and expected results as RDL fixtures, cites them by designator and example number, and records the licence basis.

## Scope

**In:**
- Example inventory by designator and example number
- Fixture encoding of inputs and published results with stated digits
- Licence-evidence record for the JCGM documents

**Out:**
- Interpretation of standards (expert work, D-009); this node supplies data and structure only

## Requirements

- **REQ-UNC.RDL.EXT.JCGM-001** Each fixture shall cite the guide designator, edition and example number and shall contain no copied guide text.
- **REQ-UNC.RDL.EXT.JCGM-002** Each fixture shall state the number of digits to which the published result is compared.

## Research hypotheses

- **H-1** LPU, MCM and multivariate engines reproduce every encoded worked example to the digits the guide publishes.

## Interfaces

To be refined when this node is expanded.

## Decomposition hints

Split along the Scope items above.

## Open questions

- Added from the research-hypothesis gap list (generated/reviews/RESEARCH-HYPOTHESES.md), 2026-11-17. Thresholds are targets marked [unverified].

## Children

_None yet: status `open`._
