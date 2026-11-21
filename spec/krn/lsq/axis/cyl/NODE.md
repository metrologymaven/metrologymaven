---
id: KRN.LSQ.AXIS.CYL
title: Least-squares cylinder
level: 4
parent: KRN.LSQ.AXIS
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.LSQ.AXIS-001, REQ-KRN.LSQ.AXIS-002, REQ-KRN.LSQ.AXIS-003, REQ-KRN.LSQ.AXIS-005, REQ-KRN.LSQ.AXIS-006]
covers: [GOM.CON.18, PW.CON.02, SA.REL.01]
depends_on: [KRN.LSQ.TRAIT, KRN.LSQ.INIT, KRN.SOLV.NLLS, KRN.SOLV.MSTART, KRN.MAN.CARD]
uses_artifacts: [UNC.RDL.FREEZE]
side_effect: W2
determinism: D0
cli: [fid fit ls-cylinder]
mcp: [fit_ls_cylinder]
journal: [fit.ls_cylinder@1]
updated: 2026-11-20
---

# KRN.LSQ.AXIS.CYL: Least-squares cylinder

## Purpose

The orthogonal-distance least-squares cylinder, the most measured axial feature and a TraCIM Gaussian primitive. It is non-convex in the axis direction, so it runs every start from `KRN.LSQ.INIT` through `KRN.SOLV.MSTART` and flags disagreement. Its result is also the start for minimum-zone, envelope and tangential cylinders.

## Scope

**In:** algorithm `fit.cylinder.ls@1.0.0`; residual r_i = dist(p_i, axis) - R; axis parameterised by two tangent-plane parameters about the current axis with the axis point re-centred to the foot of the centroid after each accepted step; constraints fixed radius and fixed axis direction; multi-start and agreement; diagnostics; the command; a spec card.
**Out:** start heuristics (`KRN.LSQ.INIT`); cone and torus (`KRN.LSQ.AXIS.CONE`, `KRN.LSQ.AXIS.TORUS`); MZ, MCC, MIC and tangential cylinders (`KRN.MZ.RND`, `KRN.ENV`).

## Requirements

- **REQ-KRN.LSQ.AXIS.CYL-001** The module shall fit a cylinder by minimising the sum of squared orthogonal distances, with the axis direction parameterised by two tangent-plane parameters about the current estimate and re-parameterised after every accepted step.
- **REQ-KRN.LSQ.AXIS.CYL-002** The fit shall run every start in the `StartSpec` from `KRN.LSQ.INIT` through `KRN.SOLV.MSTART`, select by the MSTART rule, and set `local_optimum_only` when starts disagree beyond the spec's tolerances.
- **REQ-KRN.LSQ.AXIS.CYL-003** On the PTB TraCIM Gaussian cylinder test sets and the NIST algorithm-testing cylinder datasets from the frozen RDL release, each fit shall be within 1/10 of the TraCIM default maximum permissible errors [unverified].
- **REQ-KRN.LSQ.AXIS.CYL-004** With a fixed radius, the radius shall not be a parameter; with a fixed axis direction, the problem shall reduce to a 2D circle fit of the projected points and the result shall equal `fit.circle2d.ls` on them bit for bit.
- **REQ-KRN.LSQ.AXIS.CYL-005** Each result shall report gradient norm, Jacobian rank, condition number, iteration count, residual statistics, the start list and per-start outcomes.
- **REQ-KRN.LSQ.AXIS.CYL-006** When the command is committed, the system shall journal one `fit.ls_cylinder@1` event with algorithm ID, parameter digest, input digest, seed and output digest; a dry run shall return the R1 result and journal nothing.

## Acceptance criteria

- **AC-1** Given the TraCIM Gaussian cylinder sets in RDL release `rdl/tracim-gauss-v0` (from `UNC.RDL.FREEZE` at CI time), when `fid fit ls-cylinder --json` runs on each case, then every parameter error is within 1/10 of the TraCIM default MPE [unverified].
- **AC-2** Given the NIST algorithm-testing cylinder datasets `rdl/nist-ats-v0`, then reference parameters are reproduced within the same limits [unverified].
- **AC-3** Given fixture `fixtures/fit/axis-cyl-v1` (seeded cylinders, 30-degree to full arcs, 1 km from the origin), when fitted on x86-64, ARM64 and wasm32 with the same seed, then output digests are identical; on 30-degree arcs with two plausible axes, `local_optimum_only` is set.
- **AC-4** (error path) Given 4 points, then the fit returns `TooFewPoints` and journals nothing; given coplanar points on a circle, it returns `IllConditioned` with the condition number.
- **AC-5** Given a committed run, when replayed in exact mode, then the outcome is `IDENTICAL`; the spec card check passes.

## Design

- **Registry command(s):** `fit.ls_cylinder@1`, W2 (dry-run R1). Params: point-set handle (`PointsView`), seed, optional fixed radius (m), optional fixed axis direction, `NllsConfig`, `StartSpec` overrides. Result: `FittedFeature::Cylinder`, residual statistics (m), diagnostics, multi-start summary.
- **Algorithm:** centred and scaled input (TRAIT); analytic Jacobian of distance-to-axis; `fid_solv::nlls` (Levenberg-Marquardt); `fid_solv::mstart`. References: Forbes 1989, "Least-squares best-fit geometric elements" (NPL report) [unverified citation]; Shakarji 1998 (NIST ATS fits).
- **Wrong-axis check (as built).** Multi-start agreement is not proof of correctness: every start can converge to the same wrong axis (review of #358). The reference is `plane_rms`, the RMS distance of the points to their least-squares plane. With `ratio = rms / plane_rms`: `ratio >= 1` on an unconstrained fit is `ImplausibleFit`, because a cylinder of growing radius tends to any plane, so the global least-squares cylinder is never worse than the least-squares plane and such a fit is provably a wrong local optimum; `ratio > 0.1` sets `suspect_axis` (the cylinder explains less than 99 % of the out-of-plane variance) [unverified threshold]. With a fixed radius or direction the family no longer contains every plane, so only the flag applies.
- **Errors:** `TooFewPoints`, `Coplanar`, `IllConditioned`, `NotConverged`, `Solver`, `MultiStartFailed` (code `multi_start`, every start failed), `ImplausibleFit` (code `implausible_fit`).
- **Determinism:** D0 for a given seed.
- **Performance budget:** 1 M points, 8 starts < 1.5 s [unverified until measured].
- **Crate:** `fid-fit` (module `ls_cyl`).

## Test fixtures

TraCIM Gaussian and NIST ATS cylinder datasets from the RDL release (`UNC.RDL.FREEZE`; licence and redistribution terms [unverified]; fetched at CI time, not committed). `fixtures/fit/axis-cyl-v1`: seeded generator, CC-BY-4.0. As built: `fixtures/fit/make_axis_cyl_fixtures.py` (stdlib, 60-digit Decimal, no libm) computes each case's least-squares cylinder by Gauss-Newton in Decimal on the rounded input. It covers coverage 360/180/90/60/30 degrees with axes 0.01 m, 0.3 m and 1 km from the origin, half cylinders 1.066 radii long (and a sweep 1.03 to 1.11) where the principal axes mix, fixed-radius and fixed-direction cases, 12 short noisy 30-degree arcs, a one-start wrong-axis case, 4 points and a coplanar circle.

## Build prompt

> In crate `fid-fit` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-solv`), add module `ls_cyl` implementing the `Fit` trait for `fit.cylinder.ls@1.0.0`: residuals dist(p, axis) - R on centred, scaled data; axis by two tangent-plane parameters re-parameterised after each accepted step; analytic Jacobian; `fid_solv::nlls` LM; all starts from `ls_init`'s `StartSpec` via `fid_solv::mstart` with `local_optimum_only` on disagreement; fixed radius and fixed axis direction (the latter equals `fit.circle2d.ls` on projected points bit for bit). Report diagnostics, residual statistics, starts and per-start outcomes. Add the registry data row `fit.ls_cylinder@1` (W2, dry-run R1, D0 per seed; CLI `fid fit ls-cylinder`; MCP `fit_ls_cylinder`; one journal event with algorithm ID, parameter digest, input digest, seed, output digest) and a spec card. Tests: TraCIM Gaussian and NIST ATS cylinder sets from the frozen RDL release at CI time (1/10 of TraCIM default MPE), `fixtures/fit/axis-cyl-v1` cross-target digests on x86-64/ARM64/wasm32 and partial-arc flag, error paths, exact replay IDENTICAL. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; no reference-dataset constants in source; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-5 of spec node KRN.LSQ.AXIS.CYL.

## Open questions

- None beyond the parent's.
- **Partial build (#362, `fid-fit` module `ls_cyl`, not marked built).** Done: `fit.cylinder.ls@1.0.0` with fixed radius and fixed axis direction, INIT starts through `fid_solv::mstart`, `local_optimum_only`, the wrong-axis check, diagnostics and the multi-start record (`CylinderFit`), the spec card `crates/fid-fit/cards/fit.cylinder.ls@1.0.0.toml`, the command data row `fit.ls_cylinder@1` with optional multi-start and check fields, and AC-3 and AC-4 in `crates/fid-fit/tests/ls_cyl.rs`. Remaining: (1) the registry, CLI and MCP layer and the journal event (REQ-006, AC-5), which, as for `KRN.LSQ.LIN`, waits for `CORE.REG` to read the kernel's fit data rows; (2) AC-1 and AC-2, which need the TraCIM and NIST ATS sets in the RDL (`UNC.RDL.FREEZE`); (3) REQ-004's bit-identity of the fixed-direction fit with `fit.circle2d.ls`, which waits for `KRN.LSQ.RND`. The fixed-direction fit is a three-parameter circle fit of the projections from INIT's algebraic circle and must be made to share that leaf's rules when it is built.
- **Choices made at build time (confirm in review).** (a) REQ-001 says the axis is re-parameterised after every accepted step; the NLLS driver has no per-step hook, so the axis is re-anchored (foot of the centroid, new tangent-plane frame) after each converged solve and the solve repeated, at most 8 rounds; the reported diagnostics are those about the final axis. (b) AC-4's coplanar circle returns the contract's typed `Coplanar` (with the condition number), as the Errors list allows, not `IllConditioned`. (c) Outcomes are compared by `mstart` as parameters in INIT's start frame (tangents, metres) with INIT's tolerances. (d) `seed: None` uses seed 0.
- **AC-3's "two plausible axes" as found.** On 30-degree arcs 0.2 radii long with 40 to 100 micrometres of noise, `KRN.LSQ.INIT` often starts nearly perpendicular to the true axis (85 to 90 degrees off), and the starts end on several minima. Of the 12 fixture cases, 7 are flagged `local_optimum_only` with distinct minima, 1 is flagged `suspect_axis`, 1 matches the reference unflagged and 3 are refused (`implausible_fit` or every start not converged); none is a confidently wrong fit. INIT's start on such arcs is a question for `KRN.LSQ.INIT` (not changed here).
- **Unsupported input.** INIT states a supported range only for the torus (at least 90 degrees of the major angle for fat tori); refusing input outside it belongs to `KRN.LSQ.AXIS.TORUS`. For the cylinder the wrong-axis check is the guard.

## Children

_Leaf: none._
