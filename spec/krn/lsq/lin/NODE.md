---
id: KRN.LSQ.LIN
title: Least-squares point, line and plane fits
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
depends_on: [KRN.LSQ.TRAIT, KRN.NUM.LA]
side_effect: W2
determinism: D0
cli: [fid fit ls-point, fid fit ls-line2, fid fit ls-line3, fid fit ls-plane]
mcp: [fit_ls_point, fit_ls_line2, fit_ls_line3, fit_ls_plane]
journal: [fit.ls_point@1, fit.ls_line2@1, fit.ls_line3@1, fit.ls_plane@1]
updated: 2026-11-16
---

# KRN.LSQ.LIN: Least-squares point, line and plane fits

## Purpose

Orthogonal-distance least-squares fits for point (centroid), 2D line, 3D line and plane. They have closed-form solutions through the eigen or singular-value decomposition of centred data, they are the most used features in every incumbent, and line and plane are in the TraCIM Gaussian set, so they come first.

## Scope

**In:** algorithms `fit.point.ls@1.0.0`, `fit.line2.ls@1.0.0`, `fit.line3.ls@1.0.0`, `fit.plane.ls@1.0.0`; 2D line fits in a caller-given projection plane; fixed-direction constraints (line parallel to a given direction, plane with a given normal); the four registry commands; spec cards.
**Out:** the contract and degeneracy checks (`KRN.LSQ.TRAIT`); minimum-zone and tangential lines and planes (`KRN.MZ`, `KRN.ENV`); point selection (`KRN.OUT`).

## Requirements

- **REQ-KRN.LSQ.LIN-001** `fit.point.ls` shall return the centroid computed with `KRN.NUM.RED` and residuals equal to the distances from the centroid.
- **REQ-KRN.LSQ.LIN-002** `fit.line3.ls` shall return the line through the centroid along the eigenvector of the largest eigenvalue of the centred scatter matrix, and `fit.plane.ls` the plane through the centroid with the eigenvector of the smallest eigenvalue as normal, both sign-normalised.
- **REQ-KRN.LSQ.LIN-003** `fit.line2.ls` shall project the points orthogonally onto the given plane and fit the line within it, reporting residuals as in-plane distances.
- **REQ-KRN.LSQ.LIN-004** If the two relevant eigenvalues are equal within the configured relative tolerance (no unique direction), then the fit shall return `FitError::IllConditioned` with the eigenvalue ratio.
- **REQ-KRN.LSQ.LIN-005** On the PTB TraCIM Gaussian test sets for 3D line and plane, each fit shall be within 1/10 of the TraCIM default maximum permissible errors [unverified].
- **REQ-KRN.LSQ.LIN-006** On the NIST algorithm-testing datasets for line and plane, each fit shall reproduce the reference parameters within the limits of REQ-KRN.LSQ.LIN-005 [unverified].
- **REQ-KRN.LSQ.LIN-007** Each result shall report gradient norm, Jacobian rank, condition number (eigenvalue ratio), iteration count 0 and residual statistics.
- **REQ-KRN.LSQ.LIN-008** When one of the four commands is committed, the system shall journal one `fit.ls_<primitive>@1` event carrying algorithm ID, parameter digest, input digest, seed (null) and output digest; a dry run shall return the R1 scratch result and journal nothing.

## Acceptance criteria

- **AC-1** Given the TraCIM Gaussian line and plane test sets as frozen in the RDL release `rdl/tracim-gauss-v0` (provided by `UNC.RDL` at test time), when `fid fit ls-line3 --json` and `fid fit ls-plane --json` run on each case, then every parameter error is within 1/10 of the TraCIM default MPE [unverified].
- **AC-2** Given the NIST algorithm-testing line and plane datasets `rdl/nist-ats-v0`, when the fits run, then reference parameters are reproduced within the same limits [unverified].
- **AC-3** Given fixture `fixtures/fit/lin-synth-v1` (seeded lines and planes 1 km from the origin, noise 1 micrometre), when fitted on x86-64, ARM64 and wasm32 under wasmtime, then output digests are identical across targets.
- **AC-4** (error path) Given 200 points spread evenly on a circle, when `fit.line3.ls` runs, then it returns `IllConditioned` with the eigenvalue ratio and journals nothing.
- **AC-5** Given a committed `fid fit ls-plane` on fixture data, when the journal entry is replayed in exact mode, then the outcome is `IDENTICAL`.
- **AC-6** Given each algorithm ID, then a spec card exists and CI's card check passes.

## Design

- **Registry command(s):** `fit.ls_point@1`, `fit.ls_line2@1`, `fit.ls_line3@1`, `fit.ls_plane@1`; class W2 (dry-run R1); params: element handle of a point set (resolved by the bus into a `PointsView`), optional projection plane (for line2), optional fixed direction or normal, thresholds; result: fitted feature (lengths in m, directions unitless), residual statistics (m), diagnostics.
- **Data structures:** uses `FittedFeature::{Point, Line2, Line3, Plane}` from `KRN.LSQ.TRAIT`.
- **Algorithm:** centre and scale (TRAIT); scatter matrix by fixed-order sums; `fid_num::la::sym_eig` (3 x 3); direction by eigenvector; fixed-direction variants reduce to a centroid shift (line) or offset along the normal (plane). Gradient norm and Jacobian are evaluated at the solution for diagnostics. References: the NIST ATS documentation of these fits (Shakarji 1998, "Least-squares fitting algorithms of the NIST algorithm testing system").
- **Conditioning (REQ-004):** the tolerance is `thresholds.eig_ratio_max` (contract `Thresholds`, default 0.9). With `la` the eigenvalue that defines the direction and `lb` the competing one (line3: largest and second; plane: middle and smallest, the competing one being the smallest; line2: larger and smaller), the fit is `IllConditioned` when `max(0, lb) / la > eig_ratio_max`, and that ratio is reported in the error and as `diagnostics.cond` (0 for the point and for fixed directions). Derivation of the default: by the Davis-Kahan sin-theta bound, a perturbation of the scatter matrix of spectral norm `eps * la` (rounding, or the scatter that noise adds) turns the fitted direction by at most about `eps / (1 - ratio)` radians; requiring this amplification to be at most 10 gives `ratio <= 0.9`. The ratio does not depend on the size, units or noise level of the data: a circle, ring or isotropic blob has a ratio near 1 at any noise level (1 nm and 1 um noisy circles are in `fixtures/fit/lin-synth-v1`), while a long line or flat patch has a ratio near 0 (about 1e-11 in the fixture).
- **Errors:** from `FitError`: `TooFewPoints`, `Collinear` (plane), `IllConditioned`, `NonFinite`.
- **Determinism:** D0.
- **Performance budget:** 1 M-point plane < 40 ms on reference hardware [unverified until measured].
- **Crate:** `fid-fit` (module `ls_lin`), plus the four command data rows.

## Test fixtures

TraCIM Gaussian line and plane sets and NIST ATS datasets, frozen in the RDL by `UNC.RDL` (licence and redistribution terms [unverified]; tests fetch them at CI time and do not commit them). `fixtures/fit/lin-synth-v1`: seeded synthetic generator, CC-BY-4.0.

## Build prompt

> In crate `fid-fit`, create module `ls_lin` implementing the `Fit` trait (module `contract`) for `fit.point.ls@1.0.0`, `fit.line2.ls@1.0.0`, `fit.line3.ls@1.0.0` and `fit.plane.ls@1.0.0`. Centre and scale the data; build the 3 x 3 scatter matrix with `fid_num::red`; use `fid_num::la::sym_eig`; line direction = eigenvector of the largest eigenvalue, plane normal = smallest; sign-normalise; return `IllConditioned` if the two relevant eigenvalues are equal within tolerance. line2 projects onto a given plane first. Support fixed direction or normal constraints. Report residuals, statistics and diagnostics (gradient norm, rank, condition = eigenvalue ratio, iterations 0). Add command data rows for `fit.ls_point@1`, `fit.ls_line2@1`, `fit.ls_line3@1`, `fit.ls_plane@1` (W2, dry-run R1, D0; CLI `fid fit ls-<primitive>`, MCP `fit_ls_<primitive>`; journal one event with algorithm ID, parameter digest, input digest, seed, output digest) and spec cards per algorithm. Tests: TraCIM Gaussian line/plane sets and NIST ATS sets from the RDL at CI time (within 1/10 of TraCIM default MPE), `fixtures/fit/lin-synth-v1` far-from-origin cross-target digests on x86-64/ARM64/wasm32, circle-input error path, exact replay IDENTICAL. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch, no `unsafe`, no I/O; no reference-dataset constants in source; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-6 of spec node KRN.LSQ.LIN.

## Open questions

- None beyond the parent's R1/W2 convention.
- **Partial build (`fid-fit` module `ls_lin`, not yet marked built).** Done: the four algorithms with fixed-direction and projection-plane variants, residuals, statistics and diagnostics, spec cards (`crates/fid-fit/cards/`), command data rows, and AC-3, AC-4 and AC-6 in `crates/fid-fit/tests/ls_lin.rs` against `fixtures/fit/lin-synth-v1`. Remaining: (1) the registry, CLI and MCP layer of `fit.ls_*@1` (W2 commit with R1 dry run, one journaled event per commit, REQ-008, AC-5), which needs `CORE.REG` to read the kernel's fit data rows (see `KRN.LSQ.TRAIT` open questions); (2) AC-1 and AC-2, which need the TraCIM and NIST ATS sets in the RDL (`UNC.RDL`, terms open in D-038); (3) switching from the interim in-crate cyclic Jacobi to `fid_num::la::sym_eig` once `KRN.NUM.LA.EIG` is built.
- **Choices made at build time (confirm in review).** Algorithm IDs, commands, CLI and MCP names use the contract's primitive slugs (`fit.line2.ls@1.0.0`, `fit.ls_line2@1`, `fid fit ls-line2`, `fit_ls_line2`, and `line3` likewise; this leaf said `line2d` / `line3d` before it was built) because `FitResult::assemble` requires the slug. REQ-004's tolerance is the ratio rule under Design (`thresholds.eig_ratio_max`, added to the contract's `Thresholds` and to every fit command row). Per-point weights are refused (`InvalidParams`), not implemented.
- **Note for `CORE.REG` (wire form of results).** Rows declare value types, but the canonical result bytes carry every value as a JSON string: `number` as 16 lowercase hex digits of the IEEE-754 bits, `integer` as decimal digits, arrays of those, `points` as arrays of 3-number arrays; nested objects follow the dotted field names; `feature.kind` and a `null` certificate are extra. Documented in `fid-fit` `contract::rows` and checked against real results in `crates/fid-fit/tests/ls_lin.rs`.

## Children

_Leaf: none._
