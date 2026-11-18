---
id: UNC.RDL.EXT.FILT
title: Filter softgauge importer
level: 4
parent: UNC.RDL.EXT
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.RDL.EXT-005]
covers: []
depends_on: [UNC.RDL.EXT.IMP, UNC.RDL.EXT.LIC]
uses_artifacts: [UNC.ORS.FLT]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.RDL.EXT.FILT: Filter softgauge importer

## Purpose

Gives filter hypotheses (UNC.ORS.FLT) a route to public reference data. It imports published filter softgauges and reference-software outputs into the RDL with licence evidence, or records that redistribution is not allowed and keeps only digests.

## Scope

**In:**
- Source survey and licence evidence per data set
- Importer to RDL format with source digests
- Digest-only mode when redistribution is not permitted

**Out:**
- Interpretation of standards (expert work, D-009); this node supplies data and structure only

## Requirements

- **REQ-UNC.RDL.EXT.FILT-001** Each imported data set shall carry a licence-evidence record and source digest, or be stored as digest-only when redistribution is not permitted.
- **REQ-UNC.RDL.EXT.FILT-002** Import shall be bounded and fail closed on malformed input.

## Research hypotheses

- **H-1** Project filters agree with the imported public filter references within the stated tolerance [unverified] on every imported case.

## Interfaces

To be refined when this node is expanded.

## Decomposition hints

Split along the Scope items above.

## Open questions

- Added from the research-hypothesis gap list (generated/reviews/RESEARCH-HYPOTHESES.md), 2026-11-17. Thresholds are targets marked [unverified].

## Children

_None yet: status `open`._
