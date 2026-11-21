---
id: KRN.LSQ.RND
title: Least-squares circle and sphere fits
level: 3
parent: KRN.LSQ
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.LSQ-002, REQ-KRN.LSQ-003, REQ-KRN.LSQ-004, REQ-KRN.LSQ-005, REQ-KRN.LSQ-006, REQ-KRN.LSQ-007, REQ-KRN.LSQ-009, REQ-KRN.LSQ-010]
covers: [GOM.CON.18, PW.CON.01, PW.CON.02, SA.REL.01]
depends_on: [KRN.LSQ.TRAIT, KRN.LSQ.LIN, KRN.SOLV.NLLS]
side_effect: W2
determinism: D0
cli: [fid fit ls-circle2, fid fit ls-circle3, fid fit ls-sphere]
mcp: [fit_ls_circle2, fit_ls_circle3, fit_ls_sphere]
journal: [fit.ls_circle2@1, fit.ls_circle3@1, fit.ls_sphere@1]
updated: 2026-11-21
---

# KRN.LSQ.RND: Least-squares circle and sphere fits

## Purpose

Orthogonal-distance least-squares fits for 2D circle, 3D circle and sphere. They are iterative but well behaved from an algebraic start, and 3D circle and sphere are in the TraCIM Gaussian set. Partial arcs are the classic failure point, so coverage checks and conditioning errors are part of the leaf.

## Scope

**In:** algorithms `fit.circle2d.ls@1.0.0`, `fit.circle3d.ls@1.0.0`, `fit.sphere.ls@1.0.0`; algebraic starts; a fixed-radius constraint; the three registry commands; spec cards.
**Out:** the contract, centring and coverage computation (`KRN.LSQ.TRAIT`); cylinder, cone and torus (`KRN.LSQ.AXIS`); minimum-zone and envelope circles and spheres (`KRN.MZ`, `KRN.ENV`).

## Requirements

- **REQ-KRN.LSQ.RND-001** `fit.circle2d.ls` shall project points onto a caller-given plane and minimise the sum of squared in-plane distances to the circle, starting from an algebraic circle fit.
- **REQ-KRN.LSQ.RND-002** `fit.circle3d.ls` shall minimise the sum of squared 3D distances to the circle over centre, normal and radius jointly, starting from the plane fit of `KRN.LSQ.LIN` and the in-plane algebraic circle.
- **REQ-KRN.LSQ.RND-003** `fit.sphere.ls` shall minimise the sum of squared distances to the sphere starting from an algebraic sphere fit.
- **REQ-KRN.LSQ.RND-004** Each fit shall use `KRN.SOLV.NLLS` (Levenberg-Marquardt) with a normal parameterised by two local tangent-plane parameters about the start normal, so no parameterisation singularity occurs.
- **REQ-KRN.LSQ.RND-005** If angular coverage is below the configured threshold (default 30 degrees [unverified]) or the Jacobian condition number exceeds the configured limit, then the fit shall return `InsufficientCoverage` or `IllConditioned` with coverage, condition number and point count.
- **REQ-KRN.LSQ.RND-006** When a fixed radius is given, the fit shall hold the radius at that value and fit the remaining parameters.
- **REQ-KRN.LSQ.RND-007** On the PTB TraCIM Gaussian test sets for 3D circle and sphere, and the NIST algorithm-testing circle and sphere datasets, each fit shall be within 1/10 of the TraCIM default maximum permissible errors [unverified].
- **REQ-KRN.LSQ.RND-008** When a command is committed, the system shall journal one `fit.ls_<primitive>@1` event with algorithm ID, parameter digest, input digest, seed (null) and output digest; a dry run shall journal nothing.

## Acceptance criteria

- **AC-1** Given the TraCIM Gaussian 3D circle and sphere sets in `rdl/tracim-gauss-v0`, when `fid fit ls-circle3d --json` and `fid fit ls-sphere --json` run, then every parameter error is within 1/10 of the TraCIM default MPE [unverified].
- **AC-2** Given the NIST ATS circle and sphere datasets in `rdl/nist-ats-v0`, then reference parameters are reproduced within the same limits [unverified].
- **AC-3** Given fixture `fixtures/fit/rnd-partial-v1` (seeded circles and spheres with coverage 360, 90, 45 and 20 degrees, noise 1 micrometre), when fitted, then cases at or above 45 degrees converge within 1e-6 relative radius error [unverified] and the 20-degree case returns `InsufficientCoverage`.
- **AC-4** Given AC-3 inputs on x86-64, ARM64 and wasm32 under wasmtime, then all output digests are identical.
- **AC-5** (error path) Given 2 points for a sphere, then the command fails with `TooFewPoints` and journals nothing.
- **AC-6** Given a committed `fid fit ls-sphere`, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `fit.ls_circle2d@1`, `fit.ls_circle3d@1`, `fit.ls_sphere@1`; W2 (dry-run R1); params: point-set handle, projection plane (circle2d), optional fixed radius (m), thresholds, `NllsConfig`; result: fitted feature (centre m, normal, radius m), residual statistics, diagnostics.
- **Data structures:** `FittedFeature::{Circle2, Circle3, Sphere}`.
- **Algorithm:** algebraic start (Kasa, then Pratt normalisation) by `fid_num::la::lstsq`; LM refinement through `KRN.SOLV.NLLS` with analytic Jacobians; normal by tangent-plane parameters (as in the NIST ATS fits, Shakarji 1998); coverage from `KRN.LSQ.TRAIT`.
- **Errors:** `TooFewPoints`, `InsufficientCoverage`, `IllConditioned`, `NotConverged`, `NonFinite`.
- **Determinism:** D0.
- **Performance budget:** 100 000-point sphere < 30 ms on reference hardware [unverified until measured].
- **Crate:** `fid-fit` (module `ls_rnd`), plus the three command data rows.

## Test fixtures

TraCIM Gaussian and NIST ATS sets from `UNC.RDL` at CI time (licence [unverified]). `fixtures/fit/rnd-partial-v1`: seeded synthetic generator, CC-BY-4.0.

## Build prompt

> In crate `fid-fit`, create module `ls_rnd` implementing the `Fit` trait for `fit.circle2d.ls@1.0.0`, `fit.circle3d.ls@1.0.0` and `fit.sphere.ls@1.0.0`. Centre and scale (module `contract`); algebraic start by linear least squares (`fid_num::la::lstsq`); refine with `fid_solv::nlls` (LM) using analytic Jacobians; parameterise the 3D circle normal by two tangent-plane parameters about the start normal (start from the `ls_lin` plane fit); support a fixed radius. Check coverage and conditioning and return `InsufficientCoverage` (default threshold 30 degrees) or `IllConditioned` with diagnostics. Add command data rows `fit.ls_circle2d@1`, `fit.ls_circle3d@1`, `fit.ls_sphere@1` (W2, dry-run R1, D0; CLI `fid fit ls-<primitive>`; MCP `fit_ls_<primitive>`; one journal event with algorithm ID, parameter digest, input digest, seed, output digest) and spec cards. Tests: TraCIM Gaussian 3D circle and sphere and NIST ATS circle and sphere from the RDL at CI time; `fixtures/fit/rnd-partial-v1` coverage cases; cross-target digest identity on x86-64/ARM64/wasm32; too-few-points error; exact replay IDENTICAL. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch, no `unsafe`, no I/O, no dataset constants in source; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-6 of spec node KRN.LSQ.RND.

## Open questions

- The default coverage threshold is a placeholder; it may belong in a defaults profile owned by `GDT`.
- **Partial build (`fid-fit` module `ls_rnd`, not marked built).** Done: `fit.circle2.ls@1.0.0`, `fit.circle3.ls@1.0.0` and `fit.sphere.ls@1.0.0` with the fixed-radius constraint (and the projection plane for circle2), algebraic starts, LM refinement with analytic Jacobians, the 3D circle's normal by tangent-plane parameters about the `ls_lin` plane normal, coverage and conditioning refusals, spec cards (`crates/fid-fit/cards/`), the three command data rows, and AC-3, AC-4 and the fit part of AC-5 in `crates/fid-fit/tests/ls_rnd.rs` against `fixtures/fit/rnd-partial-v1` (`make_rnd_fixtures.py`, a 60-digit Decimal Gauss-Newton reference). Remaining: (1) the registry, CLI and MCP layer and the journal event (REQ-008, AC-5's "journals nothing", AC-6), which, as for `KRN.LSQ.LIN` and `KRN.LSQ.AXIS.CYL`, waits for `CORE.REG` to read the kernel's fit data rows; (2) AC-1 and AC-2, which need the TraCIM and NIST ATS sets in the RDL (`UNC.RDL`, D-038).
- **Choices made at build time (confirm in review).** (a) Algorithm IDs, commands, CLI and MCP names use the contract's primitive slugs (`circle2`, `circle3`; this leaf said `circle2d` / `circle3d`), as `KRN.LSQ.LIN` did. (b) The contract's default coverage is 20 degrees; this leaf's 30-degree default is `ls_rnd::DEFAULT_MIN_COVERAGE`, applied by `ls_rnd::default_params()`; the fits use whatever `params.thresholds.min_coverage` holds. (c) The contract checks coverage for circles only; for the sphere the coverage is the apex angle of the smallest cone about the mean direction from the algebraic centre that holds every point (`ls_rnd::sphere_coverage`; a cap of apex angle A has coverage A, a full sphere 360 degrees). (d) The algebraic start is Kasa only, without Pratt normalisation: it is only a start, and LM reaches the same minimum on the fixture down to 45 degrees. (e) The 3D circle minimises two residuals per point (height over the plane and in-plane radial deviation), whose squares sum to the squared 3D distance, so the Jacobian stays smooth at zero distance; the reported residual is the signed 3D distance. (f) `IllConditioned` carries the condition number only (the contract's variant); coverage and point count are in `InsufficientCoverage`. (g) AC-3's "1e-6 relative radius error" is checked against the least-squares reference of the same points (the fit's convergence), not the planted radius, which the 1 micrometre noise moves by more on 45-degree arcs. (h) With a fixed radius the contract's point counts still apply (circle 3, sphere 4). (i) One solve from the start, no multi-start, so `local_optimum_only` is never set.

## Children

_Leaf: none._
