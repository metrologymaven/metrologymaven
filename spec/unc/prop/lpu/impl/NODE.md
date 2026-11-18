---
id: UNC.PROP.LPU.IMPL
title: Implicit differentiation at least-squares fit solutions
level: 4
parent: UNC.PROP.LPU
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.PROP.LPU-001, REQ-UNC.PROP.LPU-002]
covers: []
depends_on: [UNC.PROP.LPU.TAN, KRN.LSQ.TRAIT, KRN.NUM.LA.QR, KRN.NUM.RED, KRN.NUM.FN]
uses_artifacts: [UNC.RDL.FREEZE]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.PROP.LPU.IMPL: Implicit differentiation at least-squares fit solutions

## Purpose

Sensitivities of least-squares fit parameters to the input point coordinates, obtained by differentiating the optimality condition at the converged solution rather than by re-running the fit on perturbed data. The exact implicit derivative (including the residual-weighted second-derivative terms, which matter when residuals are not small) is computed from generic residual functions evaluated with hyper-dual numbers, and the linear systems are solved by QR without forming normal equations where avoidable.

## Scope

**In:**
- Generic residual functions over a `Scalar` trait (f64 and hyper-dual) for P1a least-squares primitives: line, plane, circle, sphere, cylinder, cone, in the parameterisations of `KRN.LSQ.TRAIT`
- Implicit derivative dp/dx from H dp = -B dx, with H = J_p^T J_p + sum r_i d2r_i/dp2 and B = J_p^T J_x + sum r_i d2r_i/dp dx
- Registration of one tangent provider per `fit.<primitive>.ls@<semver>`
- `no_derivative` for other LS primitives (torus, ellipse, slot, rectangle, curves) until added

**Out:**
- The fits themselves (owned by `KRN.LSQ.*`)
- Chebyshev, envelope and tangential fits (owned by `UNC.PROP.LPU.ASET`)

## Requirements

- **REQ-UNC.PROP.LPU.IMPL-001** For each supported LS primitive, the provider shall return d(parameters)/d(point coordinates) at the fit's reported solution using the exact implicit derivative including residual-weighted second-derivative terms.
- **REQ-UNC.PROP.LPU.IMPL-002** On the frozen RDL tier G least-squares cases for the supported primitives, every sensitivity shall agree with the 50-digit reference sensitivities to a relative 1e-8 [unverified].
- **REQ-UNC.PROP.LPU.IMPL-003** If H is singular or its condition estimate exceeds the fit's condition limit, then the provider shall return `ill_conditioned_sensitivity` with the estimate and compute nothing further.
- **REQ-UNC.PROP.LPU.IMPL-004** If the fit result reports a stop reason other than convergence, then the provider shall return `not_at_optimum` and no sensitivities.

## Acceptance criteria

- **AC-1** Given the tier G LS cases for line, plane, circle, sphere, cylinder and cone in the frozen release and fixture `fixtures/unc/lpu/impl/ref_sens/` (50-digit sensitivities computed offline by the RDL generator tool), when sensitivities are computed, then all agree to relative 1e-8.
- **AC-2** Given a circle case with residuals of 1 % of the radius, then the exact derivative meets AC-1 and the Gauss-Newton-only derivative (test ablation) does not.
- **AC-3** (error path) Given a cylinder fit whose result reports `IterationLimit`, then `not_at_optimum`.
- **AC-4** (error path) Given a 3-point circle on a 1e-9 rad arc, then `ill_conditioned_sensitivity`.
- **AC-5** Given the same fit result twice on x86-64, ARM64 and wasm32, then the sensitivity bytes are identical.
- **AC-H1** Hypothesis: implicit sensitivities of least-squares fit parameters achieve maximum relative entry error ≤ 1e-8 [unverified] (absolute floor 1e-12 times the largest entry [unverified]) against the 50-digit reference sensitivities on the RDL tier G least-squares families `UNC.RDL.GEN.LSNULL`, `UNC.RDL.GEN.GEOM` (partial arcs and caps, extreme aspect ratios) and `UNC.RDL.GEN.STRESS` in the frozen release `rdl-release-<semver>` whose `suite_digest` is recorded with the run, measured per case and reported per family and per coverage angle; falsified if any case exceeds the threshold.
- **AC-H2** Hypothesis: the residual-weighted second-derivative terms are necessary: on `UNC.RDL.GEN.STRESS` cases with residual RMS at least 1 % of feature size [unverified], the Gauss-Newton-only ablation exceeds the AC-H1 threshold on at least one case per primitive while the exact derivative meets it, measured by running both against the same references; falsified if the ablation meets AC-H1 on every case (the terms are then not shown necessary).

## Design

- **Registry command(s):** none; providers registered in `TangentTable`.
- **Data structures:** `trait Scalar` (f64, `HyperDual`); `trait LsResidual { fn residual<S: Scalar>(p: &[S], x: &[S; 3]) -> S; }` per primitive; `ImplicitOut { dp_dx: Mat }`.
- **Algorithm:** evaluate J_p, J_x and the second-derivative contractions point by point in point order with hyper-duals; accumulate H and B with fixed-order sums; solve H X = -B by `fid-num` pivoted QR; condition from the QR diagonal. Reference: implicit function theorem applied to the first-order optimality condition.
- **Errors:** `ill_conditioned_sensitivity{cond}`, `not_at_optimum{stop_reason}`, `no_derivative{operator}`.
- **Determinism:** D0.
- **Performance budget:** cylinder with 10 000 points in under 50 ms [unverified until measured].
- **Crate:** `fid-kunc` (module `prop::lpu::implicit`; KNP-1 applies).

## Test fixtures

Tier G LS cases from the frozen RDL release (artifact, `UNC.RDL.FREEZE`); 50-digit reference sensitivities in `fixtures/unc/lpu/impl/ref_sens/` produced by the RDL high-precision generator tool (CC-BY-4.0).

## Build prompt

> In crate `fid-kunc`, module `prop::lpu::implicit`, implement tangent providers for `fit.line.ls`, `fit.plane.ls`, `fit.circle.ls`, `fit.sphere.ls`, `fit.cylinder.ls` and `fit.cone.ls` (exact semver of the current kernel) that return d(parameters)/d(point coordinates) at the converged solution. Write generic residual functions over a `Scalar` trait (f64 and hyper-dual) in the kernel parameterisations. Form H = J_p^T J_p + sum r_i d2r_i/dp2 and B = J_p^T J_x + sum r_i d2r_i/dp dx with fixed-order sums, and solve H X = -B by pivoted QR from `fid-num`. Errors: `ill_conditioned_sensitivity{cond}` above the fit's condition limit, `not_at_optimum` when the fit did not converge, `no_derivative` for other primitives. Tests AC-1..AC-5 (tier G cases plus 50-digit references to relative 1e-8; residual-term ablation; error paths; cross-target byte identity). Constraints: KNP-1, no finite differences, CONSTITUTION 8.

## Open questions

- The 1e-8 agreement target comes from the parent and is unmeasured [unverified].

## Children

_Leaf: none._
