---
id: KRN.MZ.LIN
title: Minimum-zone lines and plane
level: 3
parent: KRN.MZ
kind: module
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-KRN.MZ-001, REQ-KRN.MZ-003, REQ-KRN.MZ-004, REQ-KRN.MZ-005, REQ-KRN.MZ-009]
covers: [GOM.CON.19, PW.CON.05]
depends_on: [KRN.LSQ.LIN, KRN.SOLV.CONIC, KRN.SOLV.SEQ, KRN.SOLV.CERT]
uses_artifacts: [UNC.RDL.FREEZE]
cli: [fid fit mz-line2d, fid fit mz-line3d, fid fit mz-plane]
mcp: [fit_mz_line2d, fit_mz_line3d, fit_mz_plane]
journal: [fit.mz_line2d@1, fit.mz_line3d@1, fit.mz_plane@1]
updated: 2026-11-17
---

# KRN.MZ.LIN: Minimum-zone lines and plane

## Purpose

Chebyshev fits for 2D line (straightness in a plane), 3D line (straightness in space, cylindrical zone) and plane (flatness): the feature minimising the maximum orthogonal deviation. They are posed as LP (2D line, plane) or SOCP (3D line) problems linearised about the least-squares solution, and their reported width is recomputed from the critical points.

## Scope

**In:**
- Formulations for `fit.line2d.mz`, `fit.line3d.mz`, `fit.plane.mz` about the least-squares start
- Active-set callbacks for `KRN.SOLV.CERT` (equal-deviation systems for the critical points)
- Certificates and recomputed zone widths
- Registry commands and spec cards

**Out** (and which node owns it instead):
- Least-squares starts (`KRN.LSQ.LIN`); solvers and certificate framework (`KRN.SOLV`)
- Straightness and flatness tolerance evaluation and filtering defaults (`GDT`)

## Requirements

- **REQ-KRN.MZ.LIN-001** The module shall compute minimum-zone 2D line, 3D line and plane fits, each implementing the `Fit` trait, starting from the least-squares solution.
- **REQ-KRN.MZ.LIN-002** The reported zone width shall be recomputed by `KRN.SOLV.CERT` from the critical point set, and the result shall carry the certificate.
- **REQ-KRN.MZ.LIN-003** On the PTB TraCIM Chebyshev 2D line and plane sets, each fit shall be within 1/10 of the TraCIM default maximum permissible errors [unverified].
- **REQ-KRN.MZ.LIN-004** When a command is committed, the system shall journal one `fit.mz_<primitive>@1` event with algorithm ID, parameter digest, input digest, seed and output digest.

## Interfaces

- **Offers:** commands `fit.mz_line2d@1`, `fit.mz_line3d@1`, `fit.mz_plane@1` (W2, dry-run R1); certificates for `GDT` straightness and flatness.
- **Consumes:** `KRN.LSQ.LIN`, `KRN.SOLV.CONIC`, `KRN.SOLV.SEQ`, `KRN.SOLV.CERT`.

## Decomposition hints

LINE2 (2D line and plane, LP), LINE3 (3D line, SOCP), CERT (active-set callbacks shared by both).

## Open questions

- TraCIM's Chebyshev set may not include the 3D line [unverified]; if not, it is validated on ORS cases only.

- The hint's shared CERT child was folded into the two fit leaves: the equal-deviation systems differ for the LP (2D line, plane) and SOCP (3D line) cases, and `KRN.SOLV.CERT` already owns the shared framework.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| KRN.MZ.LIN.FLAT | Minimum-zone 2D line and plane (LP) | 001 002 003 004 | GOM.CON.19, PW.CON.05 |
| KRN.MZ.LIN.LINE3 | Minimum-zone 3D line (SOCP) | 001 002 004 | GOM.CON.19, PW.CON.05 |
