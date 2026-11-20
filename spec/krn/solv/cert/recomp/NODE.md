---
id: KRN.SOLV.CERT.RECOMP
title: ActiveSetProblem callback and deterministic recomputation
level: 4
parent: KRN.SOLV.CERT
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-solv]
built_pr: 321
satisfies: [REQ-KRN.SOLV.CERT-002, REQ-KRN.SOLV.CERT-006]
covers: []
depends_on: [KRN.SOLV.CERT.AS, KRN.NUM.LA.QR, KRN.NUM.RED, KRN.NUM.ERR]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.SOLV.CERT.RECOMP: ActiveSetProblem callback and deterministic recomputation

## Purpose

Computes the reported optimum from the active set alone. The caller's `ActiveSetProblem` turns a candidate active set into its defining equations (for example equal deviations of the critical points); this leaf solves them with a fixed, deterministic procedure that starts from a point derived from the set itself, never from the solver's iterate. Two solver paths that reach the same set therefore give bit-identical parameters and values.

## Scope

**In:**
- The `ActiveSetProblem` trait: parameter count, expected active-set size, row labels, `start_from_set(set)`, `residuals_and_jacobian(set, params)`, `is_linear()`, `value(params)`
- Linear case: one QR least-squares solve of the active-set system
- Nonlinear case: Gauss-Newton on the active-set equations from `start_from_set`, a fixed number of steps followed by a convergence test
- Rank and condition checks on the active-set system, and the recomputed `Recomputed { params, value, cond, steps }`

**Out** (and which node owns it instead):
- Choosing the candidate sets (`KRN.SOLV.CERT.AS`) and the try order (`KRN.SOLV.CERT.CT`)
- Optimality checks (`KRN.SOLV.CERT.KKT`)
- Per-primitive implementations of the trait (`KRN.MZ`, `KRN.ENV`, `GDT.ZONE.POSE.LIN`)

## Requirements

- **REQ-KRN.SOLV.CERT.RECOMP-001** The recomputation shall take as inputs only the candidate set, the problem data held by the `ActiveSetProblem`, and fixed constants, and shall never read the conic solution or any solver iterate.
- **REQ-KRN.SOLV.CERT.RECOMP-002** When `is_linear()` is true, the module shall solve the active-set system once by `KRN.NUM.LA.QR` least squares and shall return `RankDeficient` if its rank under the recorded tolerance is below the parameter count.
- **REQ-KRN.SOLV.CERT.RECOMP-003** When `is_linear()` is false, the module shall run exactly 8 [unverified] Gauss-Newton steps on the active-set equations from `start_from_set(set)`, each solved by QR, and shall return `RecomputeNoConvergence` if the final step norm exceeds 4 u times the parameter norm.
- **REQ-KRN.SOLV.CERT.RECOMP-004** If the condition estimate of the active-set system exceeds the configured threshold (default 1e12 [unverified]), then the module shall return `IllConditioned { cond }` and no value.
- **REQ-KRN.SOLV.CERT.RECOMP-005** For the same set and data, the recomputed parameters and value shall be bit-identical regardless of the order in which the conic solution listed its rows and on x86-64, ARM64 and wasm32.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/solv/cert/recomp-v1` (straightness and flatness minimum-zone sets with exact rational optima, and circle minimum-zone sets with 50-digit optima), when recomputation runs, then linear cases are within 2·u·cond relative to the parameter scale (cond = the QR condition estimate), circles within 1e-14, and every exact optimum lies inside the proven bound.
- **AC-2** (path independence) Given one circle problem solved from two different conic starting points that reach the same active set, then the recomputed parameters and value are bit-identical.
- **AC-3** (error path) Given a candidate set of three collinear points for a straightness zone with two parameters, then `RankDeficient`; given a near-degenerate set with condition 1e14, then `IllConditioned`.
- **AC-4** Given AC-1's fixture on x86-64, ARM64 and wasm32 under wasmtime, then the digest of all outputs is identical.

## Design

- **Registry command(s):** none; library API `fid_solv::cert::recomp::{ActiveSetProblem, recompute, Recomputed, RecompConfig}`.
- **Data structures:** `trait ActiveSetProblem { fn n_params(&self) -> usize; fn expected_active(&self) -> usize; fn row_labels(&self) -> Vec<RowLabel>; fn is_linear(&self) -> bool; fn start_from_set(&self, set: &[RowLabel]) -> Result<Vec<f64>, CertError>; fn residuals_and_jacobian(&self, set: &[RowLabel], p: &[f64]) -> Result<(Vec<f64>, Mat), CertError>; fn value(&self, p: &[f64]) -> Result<f64, CertError>; fn constraint_values(&self, p: &[f64]) -> Result<Vec<f64>, CertError>; fn objective_gradient(&self, p: &[f64]) -> Result<Vec<f64>, CertError>; fn constraint_gradients(&self, set: &[RowLabel], p: &[f64]) -> Result<Mat, CertError>; }` (the last three are used by `KRN.SOLV.CERT.KKT`); `RecompConfig { rel_tol, cond_max, gn_steps }`. The trait also has three enclosure methods with `None` defaults: `residual_enclosure(set, p) -> Result<Option<Vec<Interval>>, CertError>`, `jacobian_enclosure(set, box) -> Result<Option<Vec<Interval>>, CertError>` (row-major) and `value_enclosure(box) -> Result<Option<Interval>, CertError>`; `Interval` is outward-rounded interval arithmetic. Signature: `recompute(problem, proposal: &Proposal, candidate: usize, cfg) -> Result<Recomputed, CertError>` (a set reaches it only as an uncertified candidate; a crate-internal `recompute_set` serves `KRN.SOLV.CERT.CT`). `Recomputed { set, params, value, cond, steps, param_bounds, value_bound }` with crate-private fields and accessors: `|params[i] - p*_i| <= param_bounds[i]` and `|value - value(p*)| <= value_bound` for the unique exact root `p*` in the box, proven by a Krawczyk test.
- **Algorithm:** sort the set; linear: build system, QR least squares; nonlinear: p0 = start_from_set, then fixed GN steps p <- p - lstsq(J, r), then the step-norm test; condition from the QR diagnostics.
- **Errors:** `CertError::{RankDeficient{rank}, IllConditioned{cond}, RecomputeNoConvergence{step_norm}, Callback{reason}, Numeric(NumError), BoundNotProven{reason}}` with `BoundFailure::{NotSquare, NoEnclosure, NotContracting, Overflow}`: fail closed, no result without a proven bound; an active set larger than `n_params` is solved by least squares and then fails with `NotSquare`.
- **Determinism:** D0.
- **Performance budget:** < 50 microseconds for a 7-parameter set [unverified until measured].
- **Crate:** `fid-solv` (module `cert::recomp`).

## Test fixtures

`fixtures/solv/cert/recomp-v1`: committed generator with exact or 50-digit references; CC-BY-4.0. Test-only `ActiveSetProblem` implementations for straightness, flatness and minimum-zone circle live in the test module.

## Build prompt

> In crate `fid-solv`, add module `cert::recomp` with the `ActiveSetProblem` trait (n_params, expected_active, row_labels, is_linear, start_from_set, residuals_and_jacobian, value, constraint_values, objective_gradient, constraint_gradients) and `recompute(problem, set, cfg) -> Result<Recomputed, CertError>`. Inputs are only the sorted set, the problem and constants (never solver iterates). Linear: one `fid_num::la::lstsq`; rank below n_params gives `RankDeficient`. Nonlinear: exactly `gn_steps` (default 8) Gauss-Newton steps from `start_from_set` using `lstsq`, then fail `RecomputeNoConvergence` if the last step norm > 4u * ||p||. Condition above `cond_max` (default 1e12) gives `IllConditioned`. Return `Recomputed { params, value, cond, steps }`. Tests: `fixtures/solv/cert/recomp-v1` with test implementations for straightness, flatness and minimum-zone circle, the two-start bit-identity case, error cases, cross-target digest. Constraints (CONSTITUTION §4 KNP-1, §8): only `fid-num` float primitives, no FMA, no HashMap/HashSet, no `unsafe`, no I/O; deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`. Acceptance: AC-1..AC-4 of spec node KRN.SOLV.CERT.RECOMP.

## Open questions

- `GDT.ZONE.POSE.LIN` and `KRN.MZ.LIN` were written against the open `ActiveSetProblem` interface; they must implement `start_from_set` so the nonlinear start does not depend on the iterate (for the linear GD&T rows, `is_linear() = true` and no start is needed).
- **Enclosure obligation (from KRN.SOLV.CERT.RECOMP, #321):** every `ActiveSetProblem` must implement `value_enclosure`, and a nonlinear one also `residual_enclosure` and `jacobian_enclosure`; otherwise recomputation, and so `certify`, fails with `BoundNotProven`. For a linear problem the proven bound applies to the f64 data as given (the Jacobian and residuals at p = 0 are taken as the exact system).

## Children

_Leaf: none._
