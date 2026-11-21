---
id: KRN.LSQ.AXIS.CONE
title: Least-squares cone
level: 4
parent: KRN.LSQ.AXIS
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.LSQ.AXIS-001, REQ-KRN.LSQ.AXIS-002, REQ-KRN.LSQ.AXIS-003, REQ-KRN.LSQ.AXIS-004, REQ-KRN.LSQ.AXIS-005, REQ-KRN.LSQ.AXIS-006]
covers: [GOM.CON.18, PW.CON.02, SA.REL.01]
depends_on: [KRN.LSQ.TRAIT, KRN.LSQ.INIT, KRN.SOLV.NLLS, KRN.SOLV.MSTART, KRN.MAN.CARD]
uses_artifacts: [UNC.RDL.FREEZE]
side_effect: W2
determinism: D0
cli: [fid fit ls-cone]
mcp: [fit_ls_cone]
journal: [fit.ls_cone@1]
updated: 2026-11-17
---

# KRN.LSQ.AXIS.CONE: Least-squares cone

## Purpose

The orthogonal-distance least-squares cone, a TraCIM Gaussian primitive. Cones become ill-posed near both extremes: a very shallow cone looks like a plane and a very steep one like a cylinder, and the apex is far away and poorly determined for small half-angles. The fit therefore avoids the apex in its parameters and refuses ill-posed cases with a typed error.

## Scope

**In:** algorithm `fit.cone.ls@1.0.0`; parameters: axis (two tangent-plane parameters), a reference point on the axis at the foot of the centroid, the radius at that point, and the half-angle (apex computed only for output, and omitted when beyond a recorded distance); fixed half-angle constraint; ill-posedness thresholds near 0 and near 90 degrees; multi-start and agreement; diagnostics; the command; a spec card.
**Out:** start heuristics (`KRN.LSQ.INIT`); cylinder and torus (`KRN.LSQ.AXIS.CYL`, `KRN.LSQ.AXIS.TORUS`); minimum-zone cone (`KRN.MZ.EXT`).

## Requirements

- **REQ-KRN.LSQ.AXIS.CONE-001** The module shall fit a cone by minimising the sum of squared orthogonal distances, parameterised by axis direction (two tangent-plane parameters), reference point, radius at the reference point and half-angle, with no apex parameter.
- **REQ-KRN.LSQ.AXIS.CONE-002** The fit shall run every start in the `StartSpec` through `KRN.SOLV.MSTART` and set `local_optimum_only` when starts disagree.
- **REQ-KRN.LSQ.AXIS.CONE-003** If the fitted half-angle is within the configured threshold of 0 (near-cylinder) or of 90 degrees (near-plane), then the fit shall return `IllConditioned` with the half-angle and condition number.
- **REQ-KRN.LSQ.AXIS.CONE-004** On the PTB TraCIM Gaussian cone sets and the NIST algorithm-testing cone datasets from the frozen RDL release, each fit shall be within 1/10 of the TraCIM default maximum permissible errors [unverified].
- **REQ-KRN.LSQ.AXIS.CONE-005** Each result shall report diagnostics, residual statistics, the start list, per-start outcomes and the apex only when its distance from the reference point is at most the recorded `apex_report_limit_m`.
- **REQ-KRN.LSQ.AXIS.CONE-006** When the command is committed, the system shall journal one `fit.ls_cone@1` event with algorithm ID, parameter digest, input digest, seed and output digest; a dry run shall return the R1 result and journal nothing.

## Acceptance criteria

- **AC-1** Given the TraCIM Gaussian cone sets (`rdl/tracim-gauss-v0`, from `UNC.RDL.FREEZE` at CI time), when `fid fit ls-cone --json` runs on each case, then every parameter error is within 1/10 of the TraCIM default MPE [unverified].
- **AC-2** Given the NIST ATS cone datasets `rdl/nist-ats-v0`, then reference parameters are reproduced within the same limits [unverified].
- **AC-3** (error path) Given fixture `fixtures/fit/axis-cone-v1` cases with half-angle 0.01 degree and 89.99 degrees and a 0.1 degree threshold, then the fit returns `IllConditioned` with the half-angle and journals nothing.
- **AC-4** Given the seeded cones of `fixtures/fit/axis-cone-v1` 1 km from the origin, when fitted on x86-64, ARM64 and wasm32 with the same seed, then output digests are identical.
- **AC-5** Given a committed run, when replayed in exact mode, then the outcome is `IDENTICAL`; the spec card check passes.

## Design

- **Registry command(s):** `fit.ls_cone@1`, W2 (dry-run R1). Params: point-set handle (`PointsView`), seed, optional fixed half-angle (rad), thresholds (near-0 and near-90 degrees, rad), `apex_report_limit_m`, `NllsConfig`. Result: `FittedFeature::Cone` (with reference point and radius), residual statistics, diagnostics, multi-start summary.
- **Algorithm:** distance to the cone surface in the meridian plane, analytic Jacobian, `fid_solv::nlls` LM, `fid_solv::mstart`. Reference for ill-posed wide-angle cones: ISO 10360-6:2001 test conditions (cited by designator only) [unverified clause].
- **Errors:** `TooFewPoints`, `IllConditioned`, `NotConverged`, `Solver`, `MultiStart`.
- **Determinism:** D0 for a given seed.
- **Performance budget:** 1 M points, 8 starts < 2 s [unverified until measured].
- **Crate:** `fid-fit` (module `ls_cone`).

## Test fixtures

TraCIM Gaussian and NIST ATS cone datasets from the RDL release (`UNC.RDL.FREEZE`; terms [unverified]). `fixtures/fit/axis-cone-v1`: seeded generator including extreme half-angles, CC-BY-4.0.

## Build prompt

> In crate `fid-fit` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-solv`), add module `ls_cone` implementing the `Fit` trait for `fit.cone.ls@1.0.0` with parameters axis (two tangent-plane parameters), reference point at the foot of the centroid, radius there and half-angle (no apex parameter); orthogonal distance in the meridian plane; analytic Jacobian; `fid_solv::nlls` LM; all starts via `fid_solv::mstart` with `local_optimum_only`; `IllConditioned` within explicit thresholds of 0 and 90 degrees; optional fixed half-angle; apex reported only within `apex_report_limit_m`. Add the registry data row `fit.ls_cone@1` (W2, dry-run R1, D0 per seed; CLI `fid fit ls-cone`; MCP `fit_ls_cone`; one journal event with algorithm ID, parameter digest, input digest, seed, output digest) and a spec card. Tests: TraCIM Gaussian and NIST ATS cone sets from the frozen RDL release at CI time, `fixtures/fit/axis-cone-v1` extreme half-angles and far-from-origin cross-target digests on x86-64/ARM64/wasm32, exact replay IDENTICAL. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; no reference-dataset constants in source; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-5 of spec node KRN.LSQ.AXIS.CONE.

## Open questions

- The threshold defaults for near-0 and near-90-degree half-angles are for callers to set; the clause of ISO 10360-6 that excludes wide-angle cones from testing must be checked by a domain expert [unverified].
- **Partial build (`fid-fit` module `ls_cone`, not marked built).** Done: `fit.cone.ls@1.0.0` with the apex-free parameters (tangent-plane axis, axis point, radius there, half-angle), analytic Jacobian, INIT starts through `fid_solv::mstart` and `local_optimum_only`, the fixed half-angle, the near-0 / near-90-degree thresholds and a condition limit (`IllPosedCone`, code `ill_conditioned`, with the half-angle and the condition number), the apex reported only within `apex_report_limit_m`, the wrong-axis check shared with `ls_cyl`, diagnostics and the multi-start record (`ConeFit`), the spec card `crates/fid-fit/cards/fit.cone.ls@1.0.0.toml`, the command data row `fit.ls_cone@1` with the cone's options and report fields, and in `crates/fid-fit/tests/ls_cone.rs` against `fixtures/fit/axis-cone-v1` (`make_axis_cone_fixtures.py`, a 60-digit Decimal Gauss-Newton reference): REQ-001, -002, -003, -005, AC-3's refusals (without the journal part) and AC-4's cross-target output pin, 1 km cones included. Open: (1) AC-1 and AC-2 (REQ-004), which need the TraCIM and NIST ATS cone sets in the RDL (`UNC.RDL.FREEZE`, D-038); (2) REQ-006, AC-3's "journals nothing" and AC-5 (the registry, CLI and MCP layer, the `fit.ls_cone@1` journal event and exact replay), which, as for `KRN.LSQ.AXIS.CYL`, wait for `CORE.REG` to read the kernel's fit data rows.
- **Choices made at build time (confirm in review).** (a) The contract's `FittedFeature::Cone { apex, d, half_angle }` always carries the apex (owned by `KRN.LSQ.TRAIT`); the reference point and radius are in `ConeFit::point` / `ConeFit::radius`, and the apex REQ-005 reports is `ConeFit::apex` (`None` beyond the limit). The command row's required `feature.apex` is the contract's; a contract variant `Cone { p, d, r, half_angle }` would let the canonical result omit a far apex. (b) REQ-003's error is a new contract variant `FitError::IllPosedCone { half_angle, cond }` with the existing code `ill_conditioned` (the contract's `IllConditioned` carries no half-angle). (c) The fixed half-angle and the thresholds are `ls_cone::ConeOptions`, not a contract `Constraint` (the contract has no half-angle constraint); the `Fit` trait runs with the defaults. (d) Defaults [unverified]: 0.1 degree for both thresholds (AC-3's value), a condition limit of 300 on the column-scaled Jacobian at the fit (well-posed fixture cones stay below 70; it grows like `r tan(alpha) / h` for an axial extent `h`, so it refuses short bands and narrow radial bands), and an apex report limit of 1 m. (e) The axis is re-anchored after each converged solve (at most 8 rounds), as in `ls_cyl`, because the NLLS driver has no per-step hook. (f) `seed: None` uses seed 0.
- **INIT on near-plane cones.** `KRN.LSQ.INIT`'s cone start fails on near-plane data (an 89.99-degree annulus: the start axis lies in the plane), so every start fails to converge. So that AC-3 gets a typed `IllPosedCone` with the half-angle, a start failure runs one probe solve from a plane start (the least-squares plane normal, INIT's algebraic circle of it, the half-angle from the height-radius scatter); the probe's cone is used only to refuse, never returned. Steep but well-posed cones where INIT fails the same way end as `MultiStartFailed`, a refusal; a plane-normal candidate in INIT's cone start would fix that and is a question for `KRN.LSQ.INIT`.

## Children

_Leaf: none._
