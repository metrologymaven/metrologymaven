---
id: KRN.LSQ.TRAIT
title: Fit contract, result types and degeneracy checks
level: 3
parent: KRN.LSQ
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-fit]
built_pr: 242
satisfies: [REQ-KRN.LSQ-001, REQ-KRN.LSQ-003, REQ-KRN.LSQ-006, REQ-KRN.LSQ-007, REQ-KRN.LSQ-009, REQ-KRN.LSQ-010]
covers: [PW.CON.01]
depends_on: [KRN.NUM.ERR, KRN.NUM.RED, KRN.NUM.LA, KRN.SOLV.NLLS, KRN.MAN]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# KRN.LSQ.TRAIT: Fit contract, result types and degeneracy checks

## Purpose

The contract every fitting method in the kernel implements, whatever its objective: what goes in (a point view in SI metres, typed parameters, an optional seed), what comes out (a fitted feature with residuals, statistics, diagnostics and an optional certificate, or a typed error), how inputs are centred and scaled, how degenerate inputs are detected, and how a result is encoded canonically for digests and journaling. Least-squares, minimum-zone, envelope and outlier code all build on it.

## Scope

**In:** `PointsView` (borrowed, plain-data, SI metres, optional normals); the `Fit` trait; `Primitive` and `FittedFeature` types with sign-normalised, singularity-free parameters; `FitParams` with method, constraints (fixed radius, fixed axis direction, projection plane) and convergence config; `ResidualStats`; `FitDiagnostics`; `FitError`; centring and scaling; degeneracy checks (point count, collinearity, coplanarity, angular coverage); canonical result encoding (IEEE-754 hex) and the command data-row format for `fit.*` commands.
**Out:** each method's algorithm (`KRN.LSQ.LIN`, `KRN.LSQ.RND`, `KRN.LSQ.AXIS`, `KRN.LSQ.PLAN2D`, `KRN.LSQ.CURVE`, `KRN.MZ`, `KRN.ENV`); outlier policies (`KRN.OUT`); hashing and the kernel call envelope (`KRN.MAN`); journaling (`CORE`).

## Requirements

- **REQ-KRN.LSQ.TRAIT-001** The `Fit` trait shall take a `PointsView`, `FitParams` and an optional seed, and shall return `Result<FitResult, FitError>` where `FitResult` holds the fitted feature, per-point signed residuals, residual statistics, diagnostics, an optional certificate and the algorithm ID.
- **REQ-KRN.LSQ.TRAIT-002** `PointsView` shall be a borrowed view over contiguous f64 xyz triples in SI metres with optional unit normals, and construction shall fail with a typed error on non-finite coordinates or mismatched lengths.
- **REQ-KRN.LSQ.TRAIT-003** The module shall provide centring (subtract the `KRN.NUM.RED` mean) and isotropic scaling (divide by RMS distance to the centroid) with an exact inverse map applied to fitted parameters, used by every fit.
- **REQ-KRN.LSQ.TRAIT-004** The degeneracy checks shall return `FitError::TooFewPoints`, `Collinear`, `Coplanar` or `InsufficientCoverage` with the point count, numerical rank, condition number and angular coverage, using thresholds passed in `FitParams`.
- **REQ-KRN.LSQ.TRAIT-005** `ResidualStats` shall contain RMS, maximum, minimum, mean and standard deviation of signed residuals and the point count, all computed with `KRN.NUM.RED`.
- **REQ-KRN.LSQ.TRAIT-006** `FitDiagnostics` shall contain gradient norm, Jacobian rank, condition number, iteration count and stop reason.
- **REQ-KRN.LSQ.TRAIT-007** Direction vectors in fitted features shall be unit length and sign-normalised (first non-zero component positive in total order), so equal geometry always gives equal parameters.
- **REQ-KRN.LSQ.TRAIT-008** The module shall encode a `FitResult` canonically (fixed field order, f64 as IEEE-754 hex strings, integers as strings) so `KRN.MAN` can digest it, and shall expose the algorithm ID `fit.<primitive>.<method>@<semver>`.
- **REQ-KRN.LSQ.TRAIT-009** The module shall define the data-row format that declares each `fit.<method>_<primitive>@1` registry command (parameters with quantity kind and unit, result schema, side-effect class W2 with R1 dry-run, determinism class), readable by `CORE.REG` without a crate dependency on `CORE`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/fit/trait-degenerate-v1` (12 cases: 2 points for a plane, 100 collinear points for a plane, 50 coplanar points for a sphere, a 10-degree arc for a circle, and well-posed controls), when the degeneracy checks run with the default thresholds, then each degenerate case returns the named `FitError` with point count, rank, condition number or coverage, and each control passes.
- **AC-2** Given 1 000 random points 1 km from the origin with 1 mm spread (`fixtures/fit/trait-far-origin-v1`), when centred, scaled and mapped back, then the round-trip error is at most 1 ulp of the coordinate magnitude per coordinate.
- **AC-3** Given a `FitResult` built in a test, when encoded twice and on x86-64, ARM64 and wasm32, then the bytes are identical and every f64 appears as a 16-digit hex string.
- **AC-4** (error path) Given a `PointsView` built from a buffer containing NaN at point 7, then construction returns `FitError::NonFinite{index: 7}`.
- **AC-5** Given direction vectors (0, 0, -1) and (0, 0, 1), when normalised, then both become (0, 0, 1) by bit pattern.
- **AC-6** Given the command data rows for a sample fit, when validated against the row schema, then every numeric field declares quantity kind and unit.

## Design

- **Registry command(s):** none directly; this leaf defines the row format used by every `fit.*` command.
- **Data structures:** `PointsView<'a> { xyz: &'a [f64], normals: Option<&'a [f64]> }`; `trait Fit { fn algorithm_id(&self) -> AlgorithmId; fn fit(&self, pts: &PointsView, params: &FitParams, seed: Option<u64>) -> Result<FitResult, FitError>; }`; `enum FittedFeature { Point{c}, Line2{p,d}, Line3{p,d}, Plane{p,n}, Circle2{c,r}, Circle3{c,n,r}, Sphere{c,r}, Cylinder{p,d,r}, Cone{apex,d,half_angle}, Torus{c,n,R,r}, Slot{..}, Rectangle{..}, Ellipse{..}, Paraboloid{..}, Polyline{..}, Spline{..} }` (points of axes are the foot of the perpendicular from the centroid); `FitParams { method, constraints: Vec<Constraint>, thresholds, nlls: NllsConfig }`; `FitError` wraps `NumError` and `SolverError`.
- **Algorithm:** centring and scaling as in the requirements; collinearity and coplanarity from the singular values of the centred 3 x n data (ratio thresholds); angular coverage for circle-type data as the largest angle not containing a projected point, subtracted from 360 degrees. Singular values (pinned in `fid-fit` `contract::degen`): one-sided Jacobi on the 3 x 3 triangular factor of the centred data, at most 32 sweeps over the pairs (0,1), (0,2), (1,2); a pair is skipped when `|g| <= 2^-52 * (red::norm2(c_i) * red::norm2(c_j))` (column norms from `red::norm2`, so an underflowing squared norm cannot zero the tolerance), and a rotation counts, and is applied, only if it changes a data column bit for bit, so tiny spreads (down to about 1e-300 relative) end in `Coplanar`/`Collinear`, never in a stalled `NotConverged`.
- **Errors:** `FitError::{NonFinite, TooFewPoints, Collinear, Coplanar, InsufficientCoverage, IllConditioned, NotConverged, MethodUndefined, Solver(SolverError), Num(NumError)}`.
- **Determinism:** D0.
- **Performance budget:** checks and statistics on 1 M points < 30 ms on reference hardware [unverified until measured].
- **Crate:** `fid-fit` (module `contract`).

## Test fixtures

`fixtures/fit/trait-degenerate-v1` and `fixtures/fit/trait-far-origin-v1`: seeded synthetic generators committed with the fixtures, CC-BY-4.0.

## Build prompt

> Create crate `fid-fit` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-solv` only; no dependency on any GEO or CORE crate) with module `contract`. Define `PointsView` (borrowed f64 xyz triples in SI metres, optional normals; validating constructor), the `Fit` trait (`fit(&PointsView, &FitParams, Option<u64>) -> Result<FitResult, FitError>`, `algorithm_id()` returning `fit.<primitive>.<method>@<semver>`), `FittedFeature` for point, 2D/3D line, plane, 2D/3D circle, sphere, cylinder, cone, torus, slot, rectangle, ellipse, paraboloid, polyline and spline, `FitParams` (method, constraints: fixed radius, fixed axis direction, projection plane; thresholds; `NllsConfig`), `ResidualStats` (RMS, max, min, mean, std dev, n via `fid_num::red`), `FitDiagnostics` (gradient norm, rank, cond, iterations, stop reason), `FitError`. Implement centring and isotropic scaling with exact inverse maps, degeneracy checks (count, collinear, coplanar via singular values of centred data, angular coverage), sign normalisation of directions (first non-zero component positive), and canonical encoding (fixed field order, f64 as IEEE-754 hex, integers as strings). Define a TOML or JSON data-row schema for `fit.<method>_<primitive>@1` commands (params with quantity kind and unit, result schema, side-effect W2 with R1 dry-run, determinism class) for the registry to read. Tests: `fixtures/fit/trait-degenerate-v1`, `fixtures/fit/trait-far-origin-v1`, canonical encoding identical on x86-64/ARM64/wasm32, NaN rejection, direction normalisation, row-schema validation. KNP-1: no FMA, transcendentals only via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-6 of spec node KRN.LSQ.TRAIT.

## Open questions

- The command data-row format must be accepted by `CORE.REG`, whose declaration API is "chosen at leaf level"; confirm the data-row route when `CORE.REG` is expanded.
- The `PointsView` must be producible from `GEO.TYPES` point sets without copying; confirm with GEO.

## Children

_Leaf: none._
