---
id: KRN.MZ.LIN.LINE3
title: Minimum-zone 3D line (SOCP)
level: 4
parent: KRN.MZ.LIN
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.MZ.LIN-001, REQ-KRN.MZ.LIN-002, REQ-KRN.MZ.LIN-004]
covers: [GOM.CON.19, PW.CON.05]
depends_on: [KRN.LSQ.LIN, KRN.SOLV.CONIC, KRN.SOLV.SEQ, KRN.SOLV.CERT, KRN.MAN.CARD]
uses_artifacts: [UNC.RDL.FREEZE]
side_effect: W2
determinism: D0
cli: [fid fit mz-line3d]
mcp: [fit_mz_line3d]
journal: [fit.mz_line3d@1]
updated: 2026-11-17
---

# KRN.MZ.LIN.LINE3: Minimum-zone 3D line (SOCP)

## Purpose

Straightness of an axis in space: the smallest cylinder containing all points, reported as the minimum-zone line (the cylinder's axis) and its diameter. Linearised about the least-squares line, each step is a second-order cone program (minimise r subject to the distance from each point to the axis being at most r). The zone width is recomputed from the critical points.

## Scope

**In:** algorithm `fit.line3d.mz@1.0.0`; direction by two tangent-plane parameters about the least-squares direction, axis point in the plane normal to it; sequential SOCP through `KRN.SOLV.SEQ`; active-set callbacks for `KRN.SOLV.CERT`; recomputed diameter; aspect-ratio guard (points' axial extent relative to their radial spread); the command; a spec card.
**Out:** the 2D line and plane (`KRN.MZ.LIN.FLAT`); minimum circumscribed cylinder with free radius semantics (`KRN.ENV.MCC.CYL`, which solves the same geometry for envelopes and is cross-checked against this leaf in CI).

## Requirements

- **REQ-KRN.MZ.LIN.LINE3-001** The module shall compute the minimum-zone 3D line implementing the `Fit` trait, from the least-squares 3D line, reporting the axis and the zone diameter.
- **REQ-KRN.MZ.LIN.LINE3-002** The reported diameter shall be recomputed from the critical point set at the final axis, and the result shall carry the certificate and `local_optimum_only` whenever the certificate check fails.
- **REQ-KRN.MZ.LIN.LINE3-003** If the points' axial extent is below the configured multiple of their radial spread (not line-like), then the fit shall return `IllConditioned{measure: "aspect"}`.
- **REQ-KRN.MZ.LIN.LINE3-004** On every frozen reference case for the 3D line, CI shall compare this leaf with `fit.cylinder.mcc` (`KRN.ENV.MCC.CYL`) on the same points and fail if the diameters differ by more than 1e-9 m [unverified].
- **REQ-KRN.MZ.LIN.LINE3-005** When the command is committed, the system shall journal one `fit.mz_line3d@1` event with algorithm ID, parameter digest, input digest, seed (null) and output digest; a dry run shall return the R1 result and journal nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/fit/mz-line3-v1` (seeded bent and twisted axes with known zones, 1 km from the origin), when `fid fit mz-line3d --json` runs, then diameters equal the analytic zones within 1e-10 m [unverified] and output digests are identical on x86-64, ARM64 and wasm32.
- **AC-2** Given open reference cases for 3D straightness in the frozen RDL release (`UNC.RDL.FREEZE`), then results agree with reference values within the suite tolerance [unverified; TraCIM may not include the 3D line].
- **AC-3** (error path) Given a disc-like point cloud, then the fit returns `IllConditioned{measure: "aspect"}` and journals nothing.
- **AC-4** Given a committed run, when replayed in exact mode, then the outcome is `IDENTICAL`; the spec card check passes.

## Design

- **Registry command(s):** `fit.mz_line3d@1`, W2 (dry-run R1). Params: point-set handle (`PointsView`), aspect threshold, `SeqConfig`. Result: `FittedFeature::Line3`, zone diameter (m), certificate, residual statistics.
- **Algorithm:** least-squares start; SOCP steps via `fid_solv::conic::solve_socp` inside `fid_solv::seq::solve`; `ActiveSetProblem` callbacks; certify via `fid_solv::cert::certify`.
- **Errors:** `TooFewPoints`, `IllConditioned`, `NotConverged`, `Solver`.
- **Determinism:** D0.
- **Performance budget:** 200 000 points < 400 ms [unverified until measured].
- **Crate:** `fid-fit` (module `mz_line3`).

## Test fixtures

`fixtures/fit/mz-line3-v1`: seeded generator, CC-BY-4.0. Reference cases from `UNC.RDL.FREEZE` at CI time.

## Build prompt

> In crate `fid-fit` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-solv`), add module `mz_line3` implementing the `Fit` trait for `fit.line3d.mz@1.0.0`: least-squares 3D line start, direction by two tangent-plane parameters, sequential SOCP minimise r s.t. dist(p_i, axis) <= r via `fid_solv::seq::solve` and `fid_solv::conic::solve_socp`, `ActiveSetProblem` callbacks, certificate via `fid_solv::cert::certify`, diameter recomputed from critical points, `local_optimum_only` when the check fails, `IllConditioned{measure: "aspect"}` for non-line-like data. Add a CI test comparing with `fit.cylinder.mcc` on every frozen 3D-line case (fail > 1e-9 m). Add the registry data row `fit.mz_line3d@1` (W2, dry-run R1, D0; CLI `fid fit mz-line3d`; MCP `fit_mz_line3d`; one journal event with algorithm ID, parameter digest, input digest, seed, output digest) and a spec card. Tests: `fixtures/fit/mz-line3-v1`, open reference cases from the frozen RDL release, aspect error, cross-target digests on x86-64/ARM64/wasm32, exact replay IDENTICAL. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.MZ.LIN.LINE3.

## Open questions

- The CI comparison with `KRN.ENV.MCC.CYL` is a test-time dependency only (P1a leaf against a P0 leaf); until MCC exists, the job is skipped and says so.
- **Enclosure obligation (from KRN.SOLV.CERT.RECOMP, #321):** every `ActiveSetProblem` must implement `value_enclosure`, and a nonlinear one also `residual_enclosure` and `jacobian_enclosure`; otherwise recomputation, and so `certify`, fails with `BoundNotProven`. For a linear problem the proven bound applies to the f64 data as given (the Jacobian and residuals at p = 0 are taken as the exact system).

## Children

_Leaf: none._
