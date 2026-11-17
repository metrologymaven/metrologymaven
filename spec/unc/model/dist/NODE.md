---
id: UNC.MODEL.DIST
title: Input distributions and correlation
level: 3
parent: UNC.MODEL
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.MODEL-002, REQ-UNC.MODEL-003]
covers: []
depends_on: [KRN]
side_effect: R1
determinism: D2
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# UNC.MODEL.DIST: Input distributions and correlation

## Purpose

The probability distributions an input quantity may carry, with parameter validation in SI units, their analytic expectation and standard uncertainty (used by LPU), deterministic seeded sampling (used by Monte Carlo), and the correlation-matrix check. Keeping these in one kernel module means LPU and MCM see exactly the same distribution semantics.

## Scope

**In:**
- Distributions: normal, rectangular, triangular (with mode), arcsine (U-shaped), scaled and shifted t, trapezoidal, multivariate normal
- Parameter validation with units
- Expectation and standard uncertainty for each univariate distribution
- Sampling from (seed, stream, index) through the `fid-num` counter-based RNG
- Correlation matrix validation (symmetric, unit diagonal, positive semi-definite) and correlated normal sampling

**Out:**
- Graph structure (owned by `UNC.MODEL.GRAPH`)
- The RNG itself and linear algebra (owned by `KRN.NUM`)
- Monte Carlo control (owned by `UNC.PROP.MCM`)

## Requirements

- **REQ-UNC.MODEL.DIST-001** The module shall support exactly the distributions normal(μ, σ), rectangular(a, b), triangular(a, b, c), arcsine(a, b), scaled-t(μ, s, ν), trapezoidal(a, b, β) and multivariate-normal(μ, Σ), each with parameters carrying a unit.
- **REQ-UNC.MODEL.DIST-002** If a parameter set is invalid (σ ≤ 0, b ≤ a, c outside [a, b], ν ≤ 2, β outside [0, 1], non-finite values, or units inconsistent within the distribution), then construction shall fail with error `invalid_parameter` naming the parameter.
- **REQ-UNC.MODEL.DIST-003** For every univariate distribution the module shall return the expectation and standard uncertainty from closed-form expressions, agreeing with a 50-digit reference to within 4 ulp.
- **REQ-UNC.MODEL.DIST-004** The sampler shall draw sample i of stream s from the counter-based RNG address (seed, s, i) only, so that any sample can be regenerated independently of thread count and order.
- **REQ-UNC.MODEL.DIST-005** If a correlation matrix is not exactly symmetric, has a diagonal entry other than 1, has an off-diagonal entry outside [-1, 1], or has an eigenvalue below -1e-12 times its dimension, then validation shall fail with error `invalid_correlation`.
- **REQ-UNC.MODEL.DIST-006** Given the same seed, stream and index range, the sampler shall return bit-identical samples on x86-64, ARM64 and wasm32.
- **REQ-UNC.MODEL.DIST-007** Over 10^6 samples, the sample mean and standard deviation of every univariate distribution shall lie within 5 standard errors of the closed-form values.

## Acceptance criteria

- **AC-1** Given `fixtures/unc/dist/moments.csv` (about 40 parameter sets across all distributions, with 50-digit reference expectation and standard uncertainty), when moments are computed, then each is within 4 ulp of the reference.
- **AC-2** Given each parameter set in AC-1 and seed `0xD15700001`, when 10^6 samples are drawn, then sample mean and standard deviation are within 5 standard errors of the reference, and the digest of the sample bytes equals `fixtures/unc/dist/sample_digests.txt` on wasm32, x86-64 and ARM64.
- **AC-3** (order independence) Given AC-2's draw, when samples 500,000..999,999 are drawn first and 0..499,999 second, then the concatenated bytes are identical.
- **AC-4** (error path) Given each row of `fixtures/unc/dist/invalid.csv`, when constructed, then it fails with `invalid_parameter` naming the expected parameter.
- **AC-5** (correlation) Given `fixtures/unc/dist/corr_valid.json` (3x3, 6x6) and `corr_invalid.json` (asymmetric, diagonal 0.99, indefinite with eigenvalue -0.1), when validated, then valid ones pass and each invalid one fails with `invalid_correlation`.
- **AC-6** (correlated sampling) Given a 3x3 correlation with ρ12 = 0.8 and 10^6 multivariate-normal samples, when the sample correlation is computed, then ρ12 is within 0.005 of 0.8.

## Design

- **Registry command(s):** none; kernel module used by `UNC.MODEL.GRAPH`, `UNC.PROP` and `UNC.XFM`. Algorithm IDs `unc.dist.<kind>@1.0.0`.
- **Data structures:** `enum Distribution { Normal{mu, sigma}, Rect{a, b}, Tri{a, b, c}, Arcsine{a, b}, ScaledT{mu, s, nu}, Trap{a, b, beta}, MvNormal{mu: Vec<f64>, cov: SymMatrix} }`, each value in SI with a `UnitCode`; `Correlation(SymMatrix)`.
- **Algorithm:**
  - Standard uncertainties: rectangular (b−a)/√12; triangular √((a²+b²+c²−ab−ac−bc)/18); arcsine (b−a)/(2√2); scaled-t s·√(ν/(ν−2)); trapezoidal with half-width h=(b−a)/2: h·√((1+β²)/6); normal σ.
  - Sampling by inverse transform from one uniform per draw where a closed-form inverse exists (rectangular, triangular, arcsine, trapezoidal), normal by the Wichura AS241 inverse-CDF approximation (uses only `ln` and `sqrt` from `fid-num`), scaled-t by normal over √(χ²/ν) with a Marsaglia–Tsang gamma sampler whose rejection loop consumes a fixed sub-stream per sample index.
  - Correlated normals: Cholesky with symmetric pivoting from `fid-num`, rank-revealing for PSD matrices.
  - All reductions use `fid-num` fixed-order sums.
- **Errors:** `invalid_parameter{name}`, `invalid_correlation{reason}`, numeric errors from `fid-num`.
- **Determinism:** D2 (seeded; bit-identical across targets by KNP-1).
- **Performance budget:** 10^7 univariate normal samples in under 1 s single-threaded on reference hardware [unverified until measured].
- **Crate:** `fid-kunc`.

## Test fixtures

`fixtures/unc/dist/`: `moments.csv` with references computed by a Python + mpmath script (in the oracle repository, BSD-licensed tooling, data CC-BY-4.0); `invalid.csv`; `corr_valid.json` and `corr_invalid.json`; `sample_digests.txt` generated once on the WASM reference build.

## Build prompt

> In crate `fid-kunc` (kernel; KNP-1: no FMA, transcendentals only via `fid-num`, fixed-order reductions, no std HashMap/HashSet, NaN is an error, no panics or `unwrap`), add module `dist` with `Distribution` variants normal(μ, σ), rectangular(a, b), triangular(a, b, c), arcsine(a, b), scaled-t(μ, s, ν), trapezoidal(a, b, β), multivariate-normal(μ, Σ), all parameters in SI with units.
> Provide: validating constructors (`invalid_parameter{name}` for σ ≤ 0, b ≤ a, c ∉ [a, b], ν ≤ 2, β ∉ [0, 1], non-finite, unit mismatch); `expectation()` and `std_uncertainty()` from closed forms (rect (b−a)/√12; tri √((a²+b²+c²−ab−ac−bc)/18); arcsine (b−a)/(2√2); t s√(ν/(ν−2)); trapezoid h√((1+β²)/6) with h=(b−a)/2); `sample(seed, stream, index_range)` using only the `fid-num` counter RNG address (seed, stream, i) per sample (normal by Wichura AS241 inverse CDF; t via Marsaglia–Tsang gamma with a fixed per-index sub-stream); `validate_correlation(m)` failing `invalid_correlation` for asymmetry, diagonal ≠ 1, |ρ| > 1 or min eigenvalue < −1e-12·n; correlated normal sampling via pivoted Cholesky from `fid-num`.
> Tests: moments within 4 ulp of `fixtures/unc/dist/moments.csv`; 10^6-sample mean and std within 5 standard errors; sample digests equal `sample_digests.txt` on wasm32, x86-64 and ARM64; drawing index ranges out of order gives identical bytes; every `invalid.csv` row fails with its named parameter; correlation fixtures; ρ = 0.8 recovered within 0.005. Algorithm IDs `unc.dist.<kind>@1.0.0`.

## Open questions

- The 4 ulp bound for closed-form moments assumes the `fid-num` sqrt is correctly rounded [unverified].

## Children

_Leaf: none._
