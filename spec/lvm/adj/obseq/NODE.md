---
id: LVM.ADJ.OBSEQ
title: Observation equations and Jacobians
level: 3
parent: LVM.ADJ
kind: task
status: leaf
phase: P3
size: M
owners: []
claimed_by:
satisfies: [REQ-LVM.ADJ-001]
covers: []
depends_on: [KRN.NUM, LVM.STATION.OBS]
side_effect: R1
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# LVM.ADJ.OBSEQ: Observation equations and Jacobians

## Purpose

The pure-math core of the network adjustment: for each observation type, the predicted observation from the current station pose and point coordinates, the residual, and the analytic Jacobian with respect to the station pose and the point. It also defines the generic distance-constraint observation that `LVM.SCALE` uses for reference lengths. Everything here is kernel code with no I/O; the solver (`LVM.ADJ.SOLVE`) assembles these blocks.

## Scope

**In:**
- Station-pose parameterisation: rotation matrix plus translation, with a 3-parameter incremental rotation (rotation vector) for linearisation
- Predicted value, residual and Jacobian blocks for polar, direction-only and Cartesian observations
- `DistanceConstraint` between two points: predicted length, residual and Jacobian
- Angle-residual wrapping to (−π, π]

**Out** (and which node owns it instead):
- Weights and covariance of observations (`LVM.ADJ.WEIGHT`)
- Assembly, normal equations, iteration and covariance (`LVM.ADJ.SOLVE`)
- Datum constraints (`LVM.ADJ.DATUM`)
- Image-coordinate (collinearity) equations (`LVM.ADJ.IMG`)
- Choosing which reference lengths become constraints (`LVM.SCALE.BIND`)

## Requirements

- **REQ-LVM.ADJ.OBSEQ-001** The kernel shall map a network point X to station coordinates as x = Rᵀ(X − t), where (R, t) is the station pose that maps station coordinates to the network frame.
- **REQ-LVM.ADJ.OBSEQ-002** For a polar observation the kernel shall predict hz = atan2(y, x), v = atan2(√(x² + y²), z) and d = ‖x‖; for a direction-only observation it shall predict hz and v only; for a Cartesian observation it shall predict x.
- **REQ-LVM.ADJ.OBSEQ-003** The kernel shall return residuals as observed minus predicted, with every angle residual wrapped to (−π, π].
- **REQ-LVM.ADJ.OBSEQ-004** The kernel shall return analytic Jacobian blocks with respect to the 6 station-pose increments (3 rotation-vector, 3 translation) and the 3 point coordinates, and these shall agree with central finite differences to a relative 1e-6 on the fixture set.
- **REQ-LVM.ADJ.OBSEQ-005** For a `DistanceConstraint` between points A and B the kernel shall predict L = ‖X_B − X_A‖ and return its 1×6 Jacobian with respect to both points.
- **REQ-LVM.ADJ.OBSEQ-006** If a point coincides with its station origin (‖x‖ below 1e-12 m) or a constraint's points coincide, then the kernel shall return `NET_OBSEQ_SINGULAR` instead of a value.
- **REQ-LVM.ADJ.OBSEQ-007** The kernel shall follow KNP-1: transcendental functions only through `fid-num`, no FMA, fixed evaluation order, NaN as an error.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/lvm/obseq-cases` (500 seeded random station poses, points and observation types), when the unit tests evaluate predictions at the generating truth, then every residual is below 1e-12 m for lengths and 1e-12 rad for angles.
- **AC-2** Given the same fixture, when analytic Jacobians are compared with central finite differences (step 1e-7 relative), then every entry agrees to a relative 1e-6 or an absolute 1e-9.
- **AC-3** (wrapping) Given an observed hz of +π − 1e-6 and a predicted hz of −π + 1e-6, when the residual is computed, then it is −2e-6 rad within 1e-15.
- **AC-4** (error path) Given a point at the station origin, when a polar prediction runs, then it returns `NET_OBSEQ_SINGULAR` and does not panic.
- **AC-5** (determinism) Given AC-1's inputs, when run twice on the same platform, then outputs are bit-identical; the test records output digests per platform in CI.

## Design

- **API (internal, no registry command):** `fn predict(obs: &ObsPayload, pose: &Pose, x: &[f64; 3]) -> Result<Prediction, ObsEqError>` where `Prediction { residual: SmallVec<f64; 3>, j_pose: [[f64; 6]; ≤3], j_point: [[f64; 3]; ≤3] }`; `fn predict_distance(a: &[f64; 3], b: &[f64; 3], observed: f64) -> Result<(f64, [f64; 6]), ObsEqError>`.
- **Pose:** `Pose { r: [[f64; 3]; 3], t: [f64; 3] }`; increments applied as R ← R·exp([δω]×) (right perturbation, station frame), t ← t + δt. `exp` via Rodrigues through `fid-num`.
- **Jacobians:** chain rule through x = Rᵀ(X − t): ∂x/∂X = Rᵀ, ∂x/∂t = −Rᵀ, ∂x/∂δω = [x]× for right perturbation; then polar partials ∂hz/∂x = (−y, x, 0)/ρ², ∂v/∂x = (xz, yz, −ρ²)/(ρ d²), ∂d/∂x = xᵀ/d, with ρ = √(x² + y²). Special case ρ → 0 (zenith sighting): return `NET_OBSEQ_SINGULAR` for hz.
- **Errors:** `NET_OBSEQ_SINGULAR`, `NET_OBSEQ_NAN`.
- **Determinism:** D1 (per-platform bit-exact). Becomes D0 if the `fid-num` atan2 and sqrt facade is portable bit-exact (see `KRN.NUM`).
- **Performance budget:** under 100 ns per polar observation including Jacobian on reference hardware [unverified until measured].
- **Crate:** `fid-lvm-adj` (proposed name; kernel layer, depends only on `fid-num` and kernel types).

## Test fixtures

- `fixtures/lvm/obseq-cases`: seeded generator (seed 3) of station poses, points and payloads, plus hand-written edge cases (zenith sighting, angle wrap, coincident points); CC-BY-4.0.

## Build prompt

> In the implementation repository, create the kernel-layer crate `fid-lvm-adj` (depends only on `fid-num` and kernel geometry types; no I/O). Implement observation equations for large-volume network adjustment. Station pose `(R, t)` maps station to network coordinates; a point X is seen from the station at x = Rᵀ(X − t). Provide `predict` for three payloads: polar (hz = atan2(y, x), zenith v = atan2(√(x²+y²), z), d = ‖x‖), direction-only (hz, v) and Cartesian (x), and `predict_distance` for a distance constraint L = ‖X_B − X_A‖. Return residuals observed − predicted with angle residuals wrapped to (−π, π], and analytic Jacobians w.r.t. a 6-parameter pose increment (right-perturbation rotation vector, then translation) and the 3 point coordinates. Return `NET_OBSEQ_SINGULAR` when ‖x‖ < 1e-12 m, when ρ = √(x²+y²) is below 1e-12 m for angle predictions, or when constraint points coincide; NaN in any input is `NET_OBSEQ_NAN`. Follow KNP-1 (CONSTITUTION §4): transcendentals only through `fid-num`, no FMA, fixed evaluation order, no std HashMap. No panics, no `unwrap`, no `unsafe` (CONSTITUTION §8). Tests: a seeded generator of 500 cases checking zero residual at truth (1e-12), Jacobians vs central finite differences (relative 1e-6), the angle-wrap case, the singular case and a determinism digest test. Determinism class D1.

## Open questions

- Should the zenith-sighting case (ρ → 0) use a different angle parameterisation instead of an error? Rare in practice for trackers [unverified].

## Children

_Leaf: none._
