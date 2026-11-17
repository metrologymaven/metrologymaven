---
id: ALN
title: Alignment and registration
level: 1
parent: FID
kind: domain
status: split
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-FID-001, REQ-FID-019]
covers: [GOM.ALN.*, PW.ALN.*, PCD.ALN.*, SA.XFM.*]
depends_on: [CORE, GEO, KRN]
cli: [fid align *]
mcp: [align_*]
journal: [align.*]
updated: 2026-11-16
---

# ALN: Alignment and registration

## Purpose

Every way of bringing measured data into the part or reference frame, as first-class, re-runnable alignment objects:
- prealignment and global or local best-fit (ICP family);
- 3-2-1, RPS and datum-system alignment;
- constrained fits;
- point-pair transforms with scale;
- chained alignment hierarchies;
- the sheet-metal iterative alignment.

## Scope

**In:**
- Alignment object model and hierarchy
- Automatic prealignment
- Global, local and constrained best-fit (with DoF locks and weights)
- 3-2-1, iterative 3-2-1 and RPS
- Alignment by geometric elements
- Datum-system alignment (ASME and ISO 5459; mobility-aware evaluation lives in GDT)
- Point-to-point transforms with optional scale
- Known-matrix transforms
- Picked point-pair initial alignment
- Save and recall
- Fit quality and residual statistics

**Out:**
- Multi-instrument network adjustment (LVM)
- Datum mobility in tolerance evaluation (GDT)
- Uncertainty of the transform (UNC)
- Interactive picking UI (VIZ)

## Requirements

- **REQ-ALN-001** An alignment shall be a dependency-graph element holding its method, inputs, parameters, resulting transform and residuals, and shall recompute when its inputs change.
- **REQ-ALN-002** The system shall provide automatic prealignment of measured data to CAD without user picks, plus a picked point-pair method as fallback.
- **REQ-ALN-003** The system shall provide global and local best-fit with selectable metrics (point-to-point, point-to-plane), DoF locks, per-region weights and documented convergence criteria.
- **REQ-ALN-004** The system shall provide 3-2-1, iterative 3-2-1 and RPS alignment, including the sheet-metal iterative method on vector points.
- **REQ-ALN-005** The system shall align to a datum reference frame built from datum features under the explicitly chosen ASME or ISO dialect.
- **REQ-ALN-006** Alignments shall chain into a hierarchy, and every result shall record which alignment it was evaluated in.
- **REQ-ALN-007** Each alignment shall report residuals, RMS, maximum deviation and an ill-conditioning warning, and these shall be available through CLI and MCP.
- **REQ-ALN-008** If an agent proposes an alignment strategy, then it shall be provenance-tagged and require human adoption before dependent results are released (CONSTITUTION C5).

## Interfaces

- **Offers:** `align.*` commands; the `Alignment` element type; `Transform` with residual report.
- **Consumes:** KRN (fits, optimisation), GEO (distance, frames), CORE (dependency graph).

## Decomposition hints

MODEL (alignment object, hierarchy, save/recall), PRE (prealignment and picked pairs), BFIT (global, local and constrained best-fit), RPS (3-2-1, iterative, RPS, sheet metal), DATUM (datum-system alignment), XFM (point-pair transform, scale, known matrix), QA (residuals and diagnostics).

## Split rationale

The split is by **alignment method family**, with two shared subsystems that every method writes into: the object model (MODEL) and the residual/diagnostic report (QA). The five solver children group methods by how they constrain the pose:
- PRE searches the whole pose space with no prior pose;
- BFIT minimises one global objective over surfaces or features;
- RPS locks named axes in sequence (3-2-1, RPS, vector points);
- DATUM constrains by feature type and datum precedence under a dialect;
- XFM solves from given correspondences or a given matrix in closed form.

This keeps each solver testable against its own fixture family, and keeps the C5 adoption rule and the result-stamping rule in one place (MODEL) instead of in every solver.

Alternatives considered:
- **Split by data type** (point-cloud alignment vs. feature alignment). Rejected: 3-2-1, RPS and datum methods work on both, and the residual model would be duplicated.
- **Split by incumbent workflow** (scan-inspection alignments vs. CMM alignments). Rejected: same reason the root rejected a split by app.
- **Merge QA into MODEL.** Rejected: diagnostics (conditioning, convergence history) are numerical work in the kernel layer, while MODEL is document-model work.

## Open questions

- Cycle risk with GDT: GDT depends on ALN, while ALN.DATUM needs GDT's interpretation resolver. See ALN.DATUM.
- Leapfrog (PCD.ALN.06) is shared between HAL, LVM and ALN.XFM; the single-move transform is placed in ALN.XFM.
- Prealignment and surface best-fit mainly serve scan-to-CAD (P1b); the ISO half of ALN.DATUM is P1b by the constitution. Children stay P1a until a phase decision says otherwise.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| ALN.MODEL | Alignment object model, hierarchy and save/recall | 001 006 008 | GOM.ALN.08, PCD.ALN.04, PW.ALN.08, SA.XFM.02 |
| ALN.PRE | Prealignment and picked point-pair alignment | 002 | GOM.ALN.01, GOM.ALN.11, PW.ALN.01, SA.XFM.06 |
| ALN.BFIT | Global, local and constrained best-fit | 003 | GOM.ALN.02/03/10, PCD.ALN.02, PW.ALN.02/07, SA.XFM.04 |
| ALN.RPS | 3-2-1, iterative 3-2-1, RPS and sheet-metal alignment | 004 | GOM.ALN.04/05, PCD.ALN.01/03, PW.ALN.04/05, SA.XFM.03 |
| ALN.DATUM | Element-based and datum-system alignment | 005 | GOM.ALN.06/07, PCD.ALN.05, PW.ALN.03/06 |
| ALN.XFM | Point-pair, scaled and known-matrix transforms | 001 002 | GOM.ALN.09, SA.XFM.01/05, PCD.ALN.06 |
| ALN.QA | Alignment residuals and diagnostics | 003 007 | none |
