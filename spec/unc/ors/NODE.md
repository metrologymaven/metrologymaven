---
id: UNC.ORS
title: Open reference suites for GD&T and uncertainty
level: 2
parent: UNC
kind: subsystem
status: split
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-UNC-006, REQ-UNC-002]
covers: []
depends_on: [UNC.RDL, GDT.RES, UNC.PROP]
cli: [fid rdl *-suite]
mcp: [rdl_*_suite]
journal: [rdl.run_suite]
updated: 2026-11-16
---

# UNC.ORS: Open reference suites

## Purpose

Authors and maintains the **open reference suites** (RDL tier O): cases with closed-form or independently computed expected results that exercise the supported GD&T subset and related evaluations. Suites include ORS-DRF (datum reference frames), ORS-GDT (position and material modifiers, profile, composite, simultaneous requirements), ORS-SIZE, ORS-ALN, ORS-FLT and ORS-UNC (coverage of uncertainty intervals on known-truth parts). It is separate from `UNC.RDL` because suite content requires interpretation expertise and traces to standards clauses and GDT Dialect Mapping Table (DMT) rows, while RDL owns the format and runner.

## Scope

**In:**
- Suite and case definitions with expected results and tolerances
- Traceability of each case to standard designator, edition, clause and DMT row
- ORS-DRF, ORS-GDT, ORS-SIZE, ORS-ALN, ORS-FLT, ORS-PROJ case content
- ORS-UNC: known-truth cases to test coverage of uncertainty intervals
- CI suite execution and published per-suite results

**Out** (and which node owns it instead):
- Dataset format, runner, generators and freezing (owned by `UNC.RDL`)
- The GD&T evaluation under test (owned by `GDT`)
- Public conformance dashboard rendering (owned by `OUTR`)

## Requirements

- **REQ-UNC.ORS-001** For each GD&T characteristic type in the supported subset, the suites shall contain at least one case with a closed-form expected result.
- **REQ-UNC.ORS-002** Every case shall cite the standard designator, edition and clause it exercises and, where applicable, the GDT DMT row, without reproducing standards text.
- **REQ-UNC.ORS-003** Every case shall state its expected result, the acceptance tolerance, and the dialect (ASME or ISO GPS) under which it applies.
- **REQ-UNC.ORS-004** The full ORS release shall run in CI on every merge to `main` in under 10 minutes on a standard CI runner [unverified].
- **REQ-UNC.ORS-005** ORS-UNC shall contain known-truth cases for which the empirical coverage of reported 95 % intervals over at least 1000 seeded repetitions is checked against 95 % within a stated binomial tolerance.
- **REQ-UNC.ORS-006** The suites shall be published under an open licence under every licensing outcome (CONSTITUTION C8).
- **REQ-UNC.ORS-007** When a case's expected result changes, the system shall require a new suite version and record the reason.

## Interfaces

- **Offers:** suite releases in RDL tier O format; `rdl.run_suite@1` (R1; committed run record journal `rdl.run_suite`), `rdl.list_suite@1` (R0); per-suite pass statistics for the conformance dashboard.
- **Consumes:** RDL format and runner from `UNC.RDL`; GDT evaluation commands; propagation from `UNC.PROP` for ORS-UNC.

## Decomposition hints

DRF, GDT, SIZE, ALN, FLT, PROJ, UNC (one module per suite), plus TRACE (clause and DMT citations).

## Open questions

- Which suite cases need review by a GD&T domain expert before being frozen: resolved at expansion by D-009 and `UNC.ORS.TRACE` (every case citing a DMT row needs two-expert sign-off before release).

## Split rationale

Split into **two machinery children and seven content suites**. TRACE owns the case format and the checks every suite must pass (citations, DMT rows, expert review per D-009, licence, versioning); RUN owns execution and published statistics. Each suite named in the scope (DRF, GDT, SIZE, ALN, FLT, PROJ, UNC) is its own child, because each needs different expertise and reviewers and has its own phase: FLT and PROJ start in P1b with scan filtering and CAD projection. Content children stay open: under D-009 their interpretation content is authored by experts, so an agent cannot finish them as leaves.

Alternatives considered:
- **Content by dialect** (ASME suite, ISO suite). Rejected: the same geometry and generators serve both dialects; dialect is a per-case field.
- **TRACE inside RDL.FORMAT.** Rejected: citation and DMT-row rules are ORS policy tied to GDT and D-009, while RDL's format serves every tier.
- **Fewer children by merging FLT and PROJ.** Rejected: different reviewers (filtering versus CAD geometry) and different consuming domains.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| UNC.ORS.TRACE | Suite case format, citation checks and suite versioning (leaf) | 002 003 006 007 | none |
| UNC.ORS.RUN | Suite execution in CI and published pass statistics (leaf) | 004 | none |
| UNC.ORS.DRF | ORS-DRF: datum reference frame cases | 001 002 003 | none |
| UNC.ORS.GDT | ORS-GDT: tolerance evaluation cases | 001 002 003 | none |
| UNC.ORS.SIZE | ORS-SIZE: size definition cases | 001 002 003 | none |
| UNC.ORS.ALN | ORS-ALN: alignment cases | 002 003 | none |
| UNC.ORS.FLT | ORS-FLT: filter, resampling and outlier cases | 002 003 | none |
| UNC.ORS.PROJ | ORS-PROJ: projection onto CAD faces | 002 003 | none |
| UNC.ORS.UNC | ORS-UNC: coverage of uncertainty intervals on known-truth parts | 005 | none |
