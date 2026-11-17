---
id: UNC.PROP.NLIN
title: Nonlinearity assessment for LPU
level: 3
parent: UNC.PROP
kind: module
status: open
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.PROP-002]
covers: []
depends_on: [UNC.PROP.LPU]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# UNC.PROP.NLIN: Nonlinearity assessment for LPU

## Purpose

Decides whether a first-order LPU result is trustworthy for a given model and operating point, following the nonlinearity treatment added to JCGM 100 by its 2026 amendment [unverified], and records the outcome in the uncertainty statement. It is separate from LPU because the criteria come from a newly published text that must be read and turned into a spec card first; the concept doc lists this as a P0 reading task.

## Scope

**In:**
- Spec card for the amendment's nonlinearity criteria, written from the free BIPM text in our own words
- Assessment function returning an outcome (`linear_adequate`, `second_order_needed`, `use_mcm`) and the evidence values
- Writing the outcome into the statement's nonlinearity field

**Out:**
- LPU itself (owned by `UNC.PROP.LPU`)
- Running MCM when LPU is inadequate (owned by `UNC.PROP.SELECT` and `UNC.PROP.MCM`)

## Requirements

- **REQ-UNC.PROP.NLIN-001** When LPU is evaluated, the system shall run the nonlinearity assessment and record its outcome and evidence values in the statement.
- **REQ-UNC.PROP.NLIN-002** If the assessment outcome is not `linear_adequate`, then `UNC.PROP.SELECT` shall not commit the LPU result as the reported uncertainty without an MCM evaluation.
- **REQ-UNC.PROP.NLIN-003** The assessment criteria and their version shall be recorded as `nlin@<semver>` and cite JCGM 100 and its amendment by designator and clause only [unverified: clause numbers].
- **REQ-UNC.PROP.NLIN-004** The assessment shall flag the GUM cosine-error example family (a quantity whose first derivative vanishes at the estimate) as not `linear_adequate`.

## Interfaces

- **Offers:** `assess_nonlinearity(model, x, lpu_result) -> NlinOutcome`.
- **Consumes:** LPU outputs and second-order tangents where available (UNC.PROP.LPU, KRN).

## Decomposition hints

CARD (spec card from the published text, reviewed by an uncertainty expert), ASSESS (the function and fixtures). Likely one leaf after the card exists.

## Open questions

- The amendment's exact criteria have not been read for this spec; the 2026 date and scope come from the research notes [unverified]. Read the free JCGM text and write the spec card before finishing this node.

## Children

_None yet: status `open`._
