---
id: UNC.RDL.GEN.MUT
title: Seeded-fault (mutant) catalogue
level: 4
parent: UNC.RDL.GEN
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.RDL.GEN-005]
covers: []
depends_on: [UNC.RDL.FORMAT]
uses_artifacts: [KRN.MAN.VAL, UNC.ORS.RUN]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.RDL.GEN.MUT: Seeded-fault (mutant) catalogue

## Purpose

One owner for the mutants that the research hypotheses rely on (raised by every hypothesis pass). Each mutant is a test-only variant of a fit, construction, evaluation aim or uncertainty model with a known defect class (wrong association, dropped correlation, sign error, off-by-tolerance shift). Suites report detection rates per class against this catalogue.

## Scope

**In:**
- Fault-class taxonomy (association, constraint, correlation, numeric, interpretation-structure)
- Mutant definition format and versioning inside the RDL
- Test-only build flag so mutants never ship in product builds
- Detection-report format consumed by UNC.ORS.RUN and KRN.MAN.VAL.RUN

**Out:**
- Interpretation of standards (expert work, D-009); this node supplies data and structure only

## Requirements

- **REQ-UNC.RDL.GEN.MUT-001** The catalogue shall be a versioned RDL artifact with a fault class and a defect description for every mutant.
- **REQ-UNC.RDL.GEN.MUT-002** Mutants shall build only under a test-only feature and shall never be reachable from a product build.
- **REQ-UNC.RDL.GEN.MUT-003** Every discrimination hypothesis shall cite the catalogue version and report detection per fault class.

## Research hypotheses

- **H-1** Suites built against the catalogue detect ≥ 95 % [unverified] of seeded faults per class, with zero detections on the unmutated baseline.

## Interfaces

To be refined when this node is expanded.

## Decomposition hints

Split along the Scope items above.

## Open questions

- Added from the research-hypothesis gap list (generated/reviews/RESEARCH-HYPOTHESES.md), 2026-11-17. Thresholds are targets marked [unverified].

## Children

_None yet: status `open`._
