---
id: ALN.QA
title: Alignment residuals and diagnostics
level: 2
parent: ALN
kind: subsystem
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-ALN-003, REQ-ALN-007]
covers: []
depends_on: [ALN.MODEL, KRN]
cli: [fid align get-residuals*, fid align get-diagnostics*, fid align compare-*]
mcp: [align_get_residuals*, align_get_diagnostics*, align_compare_*]
journal: []
updated: 2026-11-16
---

# ALN.QA: Alignment residuals and diagnostics

## Purpose

One residual report and diagnostic model for every alignment method: per-input residuals, RMS, maximum deviation, conditioning and DoF-observability warnings, convergence history, and comparison of two alignments. Every solver writes the same report, so CLI, MCP and reports read alignment quality the same way regardless of method. It also owns the published documentation of convergence criteria shared by iterative methods.

## Scope

**In:**
- The `ResidualReport` type: per-input residual vectors and scalars, RMS, maximum absolute deviation, count used and rejected, units
- Ill-conditioning and weak-DoF detection (for example, a rotation about a cylinder's axis left unconstrained by the data)
- Convergence history and stopping reason for iterative methods
- Comparison of two alignments (transform difference as translation and rotation)
- Read-only CLI and MCP access to all of the above

**Out** (and which node owns it instead):
- The solvers that produce the residuals (`ALN.PRE`, `ALN.BFIT`, `ALN.RPS`, `ALN.DATUM`, `ALN.XFM`)
- The uncertainty of the transform (`UNC`)
- Report layout and charts (`RPT`)
- Colour maps of residuals in the viewer (`VIZ`)

## Requirements

- **REQ-ALN.QA-001** Every alignment shall carry a residual report with per-input residuals, RMS, maximum absolute deviation, number of inputs used and number rejected, all with units.
- **REQ-ALN.QA-002** When the condition number of an alignment's normal equations (or equivalent sensitivity measure) exceeds a documented threshold (default 1e8 [unverified]), the system shall attach an ill-conditioning warning naming the weakest DoF.
- **REQ-ALN.QA-003** For every iterative method the report shall include the iteration count, the stopping criterion that fired and the objective value per iteration.
- **REQ-ALN.QA-004** The convergence criteria and defaults of every iterative alignment method shall be published in generated documentation from the registry, satisfying the documentation part of REQ-ALN-003.
- **REQ-ALN.QA-005** The residual report and diagnostics shall be retrievable through `fid align get-residuals --json` and the matching MCP tool as R0 reads, with identical content.
- **REQ-ALN.QA-006** When two alignments are compared, the system shall report the translation difference in metres and the rotation difference in radians as an R1 compute that journals nothing.
- **REQ-ALN.QA-007** Residual statistics shall be computed with fixed-order reductions under KNP-1 so that the same inputs give bit-identical statistics (D0).

## Interfaces

- **Offers:** `ResidualReport` and `AlignmentDiagnostics` types used by every solver child; `align.get_residuals` (R0), `align.get_diagnostics` (R0), `align.compare_alignments` (R1).
- **Consumes:** KRN linear algebra for conditioning; `ALN.MODEL` element references.

## Decomposition hints

REPORT (type and statistics), COND (conditioning and weak-DoF detection), HIST (convergence history and documentation), CMP (alignment comparison).

## Split rationale

The split follows the decomposition hints, one child per **kind of quality evidence**: residual statistics (REPORT), sensitivity of the solve (COND), how the iteration got there and how that is documented (HIST), and the difference between two alignments (CMP). Each has its own fixture family and its own read command, and all four are small enough to finish as leaves.

Alternatives considered:
- **One diagnostics leaf for COND and HIST.** Rejected: HIST also owns a registry-manifest schema and CI check, which is tooling work rather than kernel numerics.
- **CMP under ALN.MODEL.** Rejected: it is a read-only numeric comparison with no effect on the document, matching QA's R0/R1 surface.

## Open questions

- Should a residual report feed UNC directly as input to transform uncertainty, or should UNC re-derive it? Left to UNC's expansion.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| ALN.QA.REPORT | Residual report type, statistics and read command (leaf) | 001 005 007 | none |
| ALN.QA.COND | Conditioning and weak-DoF diagnostics (leaf) | 002 005 007 | none |
| ALN.QA.HIST | Convergence history and published convergence criteria (leaf) | 003 004 | none |
| ALN.QA.CMP | Comparison of two alignments (leaf) | 006 | none |
