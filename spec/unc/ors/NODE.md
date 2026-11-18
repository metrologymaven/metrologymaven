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
updated: 2026-11-17
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

## Research hypotheses

UNC.ORS carries the "open GD&T and datum reference suites" research risk (D-036 rule 4). The question: can open suites whose expected values are closed form or independently computed check datum, alignment, filter, projection, GD&T, size and uncertainty evaluations as tightly as the public NMI data that exists today, which covers mainly least-squares and minimum-zone fits [unverified]? Children turn the hypotheses below into `AC-H` criteria. They concern structure, coverage and discrimination power only; interpretation content and expected values that depend on reading a standard stay expert work (D-009).

- **H-1 (coverage)** The released suites contain at least one eligible case for 100 % of the characteristic types the pinned DMT marks supported and for 100 % of the registry aims they name, measured by joining suite manifests with the pinned DMT and the registry manifest; falsified if any supported row or aim has no case. Made testable by `UNC.ORS.TRACE` and each suite child.
- **H-2 (independent agreement)** Every closed-form expected value agrees with the independent oracle (`fiducial-oracles`, Python and mpmath route) within the agreement bound its suite states (1e-12 to 1e-10 of the unit, per child [unverified]), measured by the cross-check records of `UNC.RDL.GEN.XCHECK`; falsified if any released case disagrees or is still `pending`.
- **H-3 (consistency with public NMI data)** For every aim that overlaps a public NMI reference set (NIST Algorithm Testing System least-squares datasets, PTB TraCIM Gaussian and Chebyshev tests, public surface-texture filter reference software and softgauges; versions pinned by `UNC.RDL.EXT` [unverified]), an evaluation that passes the ORS cases for that aim also passes the NMI set within its published MPE; falsified if any evaluation passes ORS for an aim but fails the overlapping NMI set, which would show the ORS cases are weaker than public data.
- **H-4 (discrimination)** Each suite detects at least 95 % [unverified] of seeded single faults in a reference implementation of each aim, measured by running the suite through `UNC.ORS.RUN` against mutants of the 50-digit reference scripts, one mutant per fault class listed in the child; falsified if any suite detects fewer.
- **H-5 (calibration of uncertainty coverage)** ORS-UNC empirical coverage of 95 % intervals stays within its binomial tolerance (REQ-UNC.ORS-005); framed in `UNC.ORS.UNC`.
- **H-6 (cost)** The full release runs in CI within the REQ-UNC.ORS-004 budget; framed in `UNC.ORS.RUN`.

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
