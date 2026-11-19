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
built_in: [fid-kunc]
built_pr: 260
satisfies: [REQ-UNC.MODEL-002, REQ-UNC.MODEL-003]
covers: []
depends_on: [KRN]
side_effect: R1
determinism: D2
cli: []
mcp: []
journal: []
updated: 2026-11-19
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
- **Crate:** `fid-kunc`, module `dist`; the pivoted Cholesky is `fid_num::la::chol_pivoted` (added with this leaf).

### Pinned implementation choices (value-affecting; normative for `fid-kunc::dist`)

- **Types.** `Distribution` holds the six univariate kinds (private validated `Params` plus one `UnitCode`); the multivariate normal is the separate type `MvNormal { mu, sigma, Correlation }` with covariance `(sigma_i * sigma_j) * rho_ij`, so univariate moments cannot be asked of it. `UnitCode` is `si:m`, `si:rad` or `si:1` (the `UNC.UEC.STMT` set); a parameter is `Quantity { value, unit }`. Algorithm IDs: `unc.dist.{normal,rect,tri,arcsine,scaled_t,trap,mv_normal}@1.0.0`.
- **Validation order.** Each parameter in declaration order: finite, then unit (location and scale share the first parameter's unit; `nu` and `beta` must be `si:1`), then `|x| <= 2^510` (`PARAM_BOUND`; this applies to every parameter, including `nu`). Then relations: `sigma > 0`, `b > a`, `b - a <= 2^510` (named `b`), `a <= c <= b` (`c`), `s > 0`, `nu > 2`, `0 <= beta <= 1`. First failure is `invalid_parameter{name}`. A `beta` of `-0.0` is stored as `+0.0`. `MvNormal`: `mu` non-empty, `sigma` length, `corr` dimension, then per component `mu[i]` finite with `|mu[i]| <= 2^510`, `sigma[i]` finite with `mu[i]`'s unit and `0 < sigma[i] <= 2^510`.
- **Parameter bound (maintainer decision on #260).** A constructor must not accept a distribution that then fails on almost every draw. With every location and scale parameter, every width and `nu` at most `2^510` in magnitude, no intermediate overflows: the triangular `(u*w)*(c-a)` stays at or below `2^1020`, `red::sum([a, b, c])` at or below `3*2^510`, and the scaled-t `9*d` and `2*g` stay far below `2^1024`. `invalid.csv` carries rows just above `2^510` and at `f64::MAX` for each bound, width rows with both ends at `+-2^510`, and valid boundary rows labelled `ok`, which must construct, give finite moments and draw 2000 samples.
- **Moments.** E: `mu`; `0.5*a + 0.5*b` (rect, arcsine, trap); `red::sum([a, b, c]) / 3` (tri). u: `sigma`; `(b-a)/sqrt(12)`; tri `red::norm2([b-a, c-a, b-c]) / 6` (identical to the formula above, evaluated without cancellation); `(b-a)/sqrt(8)`; `s*sqrt(nu/(nu-2))`; `((b-a)*0.5) * sqrt((1 + beta*beta)/6)`. `sqrt` is `fid_num::f::sqrt` (correctly rounded); the 4 ulp bound held for all 40 fixture rows.
- **RNG addressing.** Every draw is one `fid_num::rng::block(seed, stream, index)` (words w0..w3). User streams are `< 2^62`; the top two stream bits name sub-streams. Primary draw of sample i, stream s: block `(seed, s, i)`, uniform from `(w1, w0)` (the `Cursor::next_u64` value). Gamma sub-stream (scaled-t): attempt `a < 256` of sample i is block `(seed, s | 2^63, i*256 + a)`, normal from `(w1, w0)`, acceptance uniform from `(w3, w2)`; `i < 2^56`. Multivariate sub-stream: normal k of sample i is block `(seed, s | 2^62, i*4096 + k)`, `(w1, w0)`; `i < 2^52`. Out-of-range stream or index is `invalid_parameter{stream|index}`. All 256 gamma attempts rejected is `sampling_exhausted` (probability below 1e-300).
- **Open uniform.** `x = hi<<32 | lo`, `u = (2*(x >> 12) + 1) * 2^-53`: never 0 or 1, and `1 - u` is exact.
- **AS241.** Wichura, Algorithm AS 241 `PPND16`, Applied Statistics 37(3), 1988, 477-484; coefficients as the shortest binary64 reprs in `fixtures/unc/dist/as241.csv` (the crate asserts equality). `q = p - 0.5`; `|q| <= 0.425`: `r = 0.180625 - q*q`, `q*A(r)/B(r)`; else `r = sqrt(-ln(min tail))`, `r <= 5`: `C(r-1.6)/D(r-1.6)`, else `E(r-5)/F(r-5)`, sign of `q`. Horner from the top coefficient, `acc*r + c_k`, no FMA. The oracle measured 5.4e-16 worst relative error of this evaluation in binary64 against a 100-digit inverse normal; the crate test bound is 1e-15.
- **Transforms.** normal `mu + sigma*ppnd16(u)`; rect `a + (b-a)*u`; tri `w = b-a`, `fc = (c-a)/w`, `u < fc`: `a + sqrt((u*w)*(c-a))`, else `b - sqrt(((1-u)*w)*(b-c))`; arcsine `h = (b-a)*0.5`, `m = a+h`, `m - h*cos(pi*u)`; trap `h`, `m` as arcsine, `f1 = (1-beta)/(2*(1+beta))`, `w = (1-beta)*(1+beta)`, `y = -1 + sqrt((2u)*w)` if `u < f1`, `(u-0.5)*(1+beta)` if `u <= 1-f1`, else `1 - sqrt((2(1-u))*w)`, result `m + h*y`; scaled-t `z = ppnd16(u)`, `g` = gamma(`nu*0.5`), `mu + s*(z/sqrt((2g)/nu))`. Gamma (Marsaglia and Tsang 2000): `d = alpha - 1/3`, `c = 1/sqrt(9d)`, `t = 1 + c*x` (reject `t <= 0`), `v = (t*t)*t`, accept `d*v` if `u_b < 1 - 0.0331*(x2*x2)` or `ln(u_b) < 0.5*x2 + d*((1-v) + ln v)`.
- **Correlation check (eigenvalue tolerance method).** Order: shape (`n = 0`, `n > 4096`, length) `shape`; first non-finite `non_finite`; `C[i][j] != C[j][i]` `asymmetric`; `C[i][i] != 1` `diagonal`; `|C[i][j]| > 1` `off_diagonal_range`. Then `tau = 1e-12*n`, `B = C` with diagonal `1 + tau`; `min eig(C) >= -tau` iff `B` is PSD, decided by `chol_pivoted(B, 0)`: rank `n` passes; otherwise the trailing Schur complement `S` (from `L` and `perm`, `red::dot`) must have every `|S_ij| <= n*2^-50`, else `indefinite`. No eigensolver is needed (`KRN.NUM.LA.EIG` is not built); the decision is exact up to rounding of order `n*2^-52`, far inside `tau`.
- **Pivoted Cholesky (`fid_num::la::chol_pivoted`).** Lower triangle only; at step k the remaining diagonals `d_j = a(perm j, perm j) - red::sum_sq(L[j][0..k])` are recomputed, the first largest is the pivot, `thr = rel_tol * d_max` at step 0, stop when `d_p <= 0` or `d_p <= thr` (rank = k), swap perm and computed rows, `L[k][k] = sqrt(d_p)`, `L[i][k] = (a(perm i, perm k) - red::dot(L[i][0..k], L[k][0..k])) / L[k][k]`.
- **Correlated normals.** Factor `chol_pivoted(R, n*2^-52)` with rank r; sample i draws `z[0..r]` from the multivariate sub-stream; `x[perm p] = mu + sigma * red::dot(L[p][0..r], z)`; output row-major per sample.
- **Statistical test (AC-2).** Standard errors come from the reference: `SE(mean) = u/sqrt(N)` and `SE(sd) = u*sqrt((kurtosis - 1)/(4N))`, with the kurtosis taken from `moments.csv`. The mean is tested for every row, because it needs only a finite variance (scaled-t `nu > 2`). The standard deviation is tested for every row with a finite fourth moment, which the delta-method SE formula needs (scaled-t `nu > 4`). So the scaled-t rows with `nu` = 2.5 and 3 are tested on the mean only, and every other row on both. Observed z-scores for the scaled-t rows at seed `0xD15700001`:

  | nu | mean z | sd z |
  |---|---|---|
  | 2.5 | -1.08 | – |
  | 3 | -0.53 | – |
  | 5 | -0.67 | -1.27 |
  | 9 | -0.77 | -1.28 |
  | 10 | -0.78 | -1.40 |
  | 30 | -0.84 | -1.42 |
  | 100 | -0.83 | -1.31 |

  All are well inside the 5-SE bound, so no stricter gate is needed.
- **`sample_digests.txt`.** SHA-256 (`fid_kci::kci::sha256_digest`) of the little-endian f64 bytes of samples `0..10^6`, seed `0xD15700001`, stream 0, one line per `moments.csv` row plus `mv_rho12` (AC-6 draw). Produced with `UNC_DIST_PRINT_DIGESTS=1 cargo test -p fid-kunc --test dist -- --nocapture` (first run on x86-64 Windows, no wasm32 toolchain locally) and asserted unchanged by the same test on wasm32-wasip1 under wasmtime (CI job `wasm32`), x86-64 Linux/Windows and ARM64 macOS. A digest is a cross-target identity pin, not a correctness reference.

## Test fixtures

`fixtures/unc/dist/` (synthetic parameter sets, CC-BY-4.0): `make_fixtures.py` (stdlib only: `fractions` for exact rational moments of the piecewise-linear densities and textbook moments, `decimal` at 80 to 100 digits for square roots and the inverse normal) writes `moments.csv` (40 rows, 50-digit references and kurtosis), `invalid.csv` (hand-labelled), `corr_valid.json` and `corr_invalid.json` (labels confirmed by an exact rational PSD test of `C + tau I`), `as241.csv` and `ppnd.csv`; `--check` compares without writing. `sample_digests.txt` is produced as pinned in the Design.

## Build prompt

> In crate `fid-kunc` (kernel; KNP-1: no FMA, transcendentals only via `fid-num`, fixed-order reductions, no std HashMap/HashSet, NaN is an error, no panics or `unwrap`), add module `dist` with `Distribution` variants normal(μ, σ), rectangular(a, b), triangular(a, b, c), arcsine(a, b), scaled-t(μ, s, ν), trapezoidal(a, b, β), multivariate-normal(μ, Σ), all parameters in SI with units.
> Provide: validating constructors (`invalid_parameter{name}` for σ ≤ 0, b ≤ a, c ∉ [a, b], ν ≤ 2, β ∉ [0, 1], non-finite, unit mismatch); `expectation()` and `std_uncertainty()` from closed forms (rect (b−a)/√12; tri √((a²+b²+c²−ab−ac−bc)/18); arcsine (b−a)/(2√2); t s√(ν/(ν−2)); trapezoid h√((1+β²)/6) with h=(b−a)/2); `sample(seed, stream, index_range)` using only the `fid-num` counter RNG address (seed, stream, i) per sample (normal by Wichura AS241 inverse CDF; t via Marsaglia–Tsang gamma with a fixed per-index sub-stream); `validate_correlation(m)` failing `invalid_correlation` for asymmetry, diagonal ≠ 1, |ρ| > 1 or min eigenvalue < −1e-12·n; correlated normal sampling via pivoted Cholesky from `fid-num`.
> Tests: moments within 4 ulp of `fixtures/unc/dist/moments.csv`; 10^6-sample mean and std within 5 standard errors; sample digests equal `sample_digests.txt` on wasm32, x86-64 and ARM64; drawing index ranges out of order gives identical bytes; every `invalid.csv` row fails with its named parameter; correlation fixtures; ρ = 0.8 recovered within 0.005. Algorithm IDs `unc.dist.<kind>@1.0.0`.

## Open questions

- The 4 ulp bound for closed-form moments assumes the `fid-num` sqrt is correctly rounded. **Resolved at build:** `fid_num::f::sqrt` is the IEEE-754 square root, and all 40 fixture rows meet the bound.

## Children

_Leaf: none._
