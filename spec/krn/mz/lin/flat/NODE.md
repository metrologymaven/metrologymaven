---
id: KRN.MZ.LIN.FLAT
title: Minimum-zone 2D line and plane (LP)
level: 4
parent: KRN.MZ.LIN
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.MZ.LIN-001, REQ-KRN.MZ.LIN-002, REQ-KRN.MZ.LIN-003, REQ-KRN.MZ.LIN-004]
covers: [GOM.CON.19, PW.CON.05]
depends_on: [KRN.LSQ.LIN, KRN.SOLV.CONIC, KRN.SOLV.SEQ, KRN.SOLV.CERT, KRN.MAN.CARD]
uses_artifacts: [UNC.RDL.FREEZE]
side_effect: W2
determinism: D0
cli: [fid fit mz-line2d, fid fit mz-plane]
mcp: [fit_mz_line2d, fit_mz_plane]
journal: [fit.mz_line2d@1, fit.mz_plane@1]
updated: 2026-11-17
---

# KRN.MZ.LIN.FLAT: Minimum-zone 2D line and plane (LP)

## Purpose

Chebyshev fits for straightness in a plane (2D line) and flatness (plane): the pair of parallel lines or planes with the smallest separation that contains all points. Linearised about the least-squares solution they become small LPs; the reported width is recomputed from the critical points (3 for a 2D line, 4 for a plane in the generic case), never taken from the solver's last iterate. Both are in the TraCIM Chebyshev set.

## Scope

**In:** algorithms `fit.line2d.mz@1.0.0` and `fit.plane.mz@1.0.0`; direction or normal parameterised by tangent-plane parameters about the least-squares solution; sequential LP (minimise t subject to -t/2 <= d_i <= t/2) through `KRN.SOLV.SEQ`; active-set callbacks for `KRN.SOLV.CERT` (equal-deviation system for the critical points: two on one side and one on the other for a 2D line; 3-1 or 2-2 for a plane); recomputed width; the two commands; spec cards.
**Out:** the 3D line (`KRN.MZ.LIN.LINE3`); least-squares starts (`KRN.LSQ.LIN`); straightness and flatness evaluation with filters (`GDT`).

## Requirements

- **REQ-KRN.MZ.LIN.FLAT-001** The module shall compute the minimum-zone 2D line (points projected into a caller-given plane) and plane, implementing the `Fit` trait, from the least-squares solution.
- **REQ-KRN.MZ.LIN.FLAT-002** The reported zone width shall be recomputed by `KRN.SOLV.CERT` from the critical point set by solving its equal-deviation system, and the result shall carry the certificate (critical points, multipliers, local optimality outcome).
- **REQ-KRN.MZ.LIN.FLAT-003** On the PTB TraCIM Chebyshev 2D line and plane sets from the frozen RDL release, each fit shall be within 1/10 of the TraCIM default maximum permissible errors [unverified].
- **REQ-KRN.MZ.LIN.FLAT-004** Because both problems are convex after the reduction to the normal's tangent-plane parameters near the optimum [unverified for large tilts], the result shall carry `local_optimum_only: false` only when the certificate's optimality check passes, and `true` otherwise.
- **REQ-KRN.MZ.LIN.FLAT-005** When a command is committed, the system shall journal one `fit.mz_<primitive>@1` event with algorithm ID, parameter digest, input digest, seed (null) and output digest; a dry run shall return the R1 result and journal nothing.

## Acceptance criteria

- **AC-1** Given the TraCIM Chebyshev 2D line and plane sets in RDL release `rdl/tracim-cheb-v0` (from `UNC.RDL.FREEZE` at CI time), when `fid fit mz-line2d --json` and `fid fit mz-plane --json` run on each case, then every parameter and the zone width are within 1/10 of the TraCIM default MPE [unverified].
- **AC-2** Given fixture `fixtures/fit/mz-flat-v1` (seeded flat and straight features with known zones, 1 km from the origin), when fitted on x86-64, ARM64 and wasm32, then output digests are identical and widths equal the analytic zones within 1e-12 m.
- **AC-3** Given a result, then its width equals the width recomputed independently from its critical points bit for bit.
- **AC-4** (error path) Given collinear points for a plane, then the fit returns `Collinear` and journals nothing.
- **AC-5** Given a committed run, when replayed in exact mode, then the outcome is `IDENTICAL`; spec card checks pass.

## Design

- **Registry command(s):** `fit.mz_line2d@1`, `fit.mz_plane@1`; W2 (dry-run R1). Params: point-set handle (`PointsView`), projection plane (line2d), `SeqConfig`. Result: `FittedFeature::{Line2, Plane}` (mid-zone feature), zone width (m), certificate, residual statistics.
- **Algorithm:** least-squares start; LP steps via `fid_solv::conic::solve_lp` inside `fid_solv::seq::solve`; `ActiveSetProblem` implementation for the equal-deviation systems; certify via `fid_solv::cert::certify`.
- **Errors:** `TooFewPoints`, `Collinear`, `NotConverged`, `Solver`.
- **Determinism:** D0.
- **Performance budget:** 1 M-point plane < 400 ms [unverified until measured].
- **Crate:** `fid-fit` (module `mz_flat`).

## Test fixtures

TraCIM Chebyshev sets from the RDL release (`UNC.RDL.FREEZE`; terms [unverified]; fetched at CI time). `fixtures/fit/mz-flat-v1`: seeded generator, CC-BY-4.0.

## Build prompt

> In crate `fid-fit` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-solv`), add module `mz_flat` implementing the `Fit` trait for `fit.line2d.mz@1.0.0` (points projected into a given plane) and `fit.plane.mz@1.0.0`: least-squares start, direction or normal by tangent-plane parameters, sequential LP minimise t s.t. |d_i| <= t/2 via `fid_solv::seq::solve` and `fid_solv::conic::solve_lp`, `ActiveSetProblem` callbacks for the equal-deviation systems (2-1 for a line; 3-1 or 2-2 for a plane), certificate via `fid_solv::cert::certify`, width recomputed from critical points, `local_optimum_only` from the certificate check. Add registry data rows `fit.mz_line2d@1`, `fit.mz_plane@1` (W2, dry-run R1, D0; CLI `fid fit mz-<primitive>`; MCP `fit_mz_<primitive>`; one journal event with algorithm ID, parameter digest, input digest, seed, output digest) and spec cards. Tests: TraCIM Chebyshev 2D line and plane sets from the frozen RDL release at CI time (1/10 of TraCIM default MPE), `fixtures/fit/mz-flat-v1` cross-target digests on x86-64/ARM64/wasm32, width recomputation identity, collinear error, exact replay IDENTICAL. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; no reference-dataset constants in source; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-5 of spec node KRN.MZ.LIN.FLAT.

## Open questions

- None beyond the parent's.

## Children

_Leaf: none._
