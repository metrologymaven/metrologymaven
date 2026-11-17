---
id: UNC.PROP.LPU.COMB
title: Graph accumulation, covariance combination and coverage factor
level: 4
parent: UNC.PROP.LPU
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.PROP.LPU-003, REQ-UNC.PROP.LPU-005]
covers: []
depends_on: [UNC.PROP.LPU.TAN, UNC.MODEL.GRAPH, UNC.MODEL.DIST, KRN.MAN.KCI, KRN.NUM.RED, KRN.NUM.FN]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.PROP.LPU.COMB: Graph accumulation, covariance combination and coverage factor

## Purpose

The LPU engine function `propagate_lpu(model, inputs, profile) -> LpuResult`. It evaluates the model graph once at the input estimates through the kernel call interface, chains operator Jacobians from `TangentTable` in topological order into the sensitivity of each scalar output to each model input, combines them with the full input covariance in a fixed-order reduction, applies the coverage-factor profile, and turns kink hints into the `lpu_not_applicable` flag.

## Scope

**In:**
- Forward accumulation of Jacobians over the graph in node-ID topological order
- Input covariance from distribution standard uncertainties (`UNC.MODEL.DIST`) and declared correlations
- u_c^2 = c^T V c, fixed order
- Profile `lpu-profile@1`: `k_mode = fixed` (default k = 2) or `welch_satterthwaite` (requires degrees of freedom on every type-A input, else error), coverage probability 0.95
- Kink evaluation: a kink with margin below `kink_factor` (default 3 [unverified]) times the propagated standard uncertainty of the kinked quantity sets `lpu_not_applicable`
- `LpuResult { y, u_c, k, U, sensitivities, covariance_digest, flags, profile }`

**Out:**
- Tangent providers (owned by `UNC.PROP.LPU.TAN`, `TCONS`, `TFORM`, `IMPL`, `ASET`)
- Vector outputs with joint covariance (owned by `UNC.PROP.MVAR`)
- Nonlinearity assessment (owned by `UNC.PROP.NLIN`)

## Requirements

- **REQ-UNC.PROP.LPU.COMB-001** The engine shall compute each output's sensitivity to each model input by chaining operator Jacobians in topological order with ties broken by node ID.
- **REQ-UNC.PROP.LPU.COMB-002** The engine shall compute u_c^2 as c^T V c over the full input covariance, including declared correlations, with fixed-order reductions from `fid-num`.
- **REQ-UNC.PROP.LPU.COMB-003** When the profile's `k_mode` is `fixed`, the engine shall use the profile's k (default 2), and when it is `welch_satterthwaite`, it shall compute effective degrees of freedom and k for the profile's coverage probability, failing with `dof_missing` if a type-A input has no declared degrees of freedom.
- **REQ-UNC.PROP.LPU.COMB-004** If any kink hint's margin is below `kink_factor` times the propagated standard uncertainty of the kinked quantity, then the result shall carry flag `lpu_not_applicable` naming the operator.
- **REQ-UNC.PROP.LPU.COMB-005** Given the same model digest, inputs and profile, the engine shall return byte-identical `LpuResult` encodings on x86-64, ARM64 and wasm32 (D0).

## Acceptance criteria

- **AC-1** Given `fixtures/unc/lpu/comb/gauge_block.json` (a linear length model of 5 inputs with one declared correlation and closed-form u_c), when propagated, then u_c matches the closed form to relative 1e-14 and U = 2 u_c.
- **AC-2** Given the same model with profile `welch_satterthwaite` and declared degrees of freedom, then the effective degrees of freedom and k match `fixtures/unc/lpu/comb/ws_expected.json` to 1e-12.
- **AC-3** (error path) Given `welch_satterthwaite` with a type-A input lacking degrees of freedom, then `dof_missing` naming the input.
- **AC-4** Given a formula `abs(A - B)` model with A - B = 1e-7 m and u(A - B) = 1e-6 m, then `lpu_not_applicable` is set.
- **AC-5** Given AC-1 on three targets, then results are byte-identical.

## Design

- **Registry command(s):** none (engine function used by `UNC.PROP.LPU.CMD`, `UNC.PROP.SELECT`, `UNC.PROP.NLIN`).
- **Data structures:** `LpuProfile { version, k_mode, k, p, kink_factor }`; `LpuResult` as in Scope with canonical encoding (f64 as IEEE-754 hex).
- **Algorithm:** one forward evaluation of the graph via `evaluate`; per node, Jacobian from the table; chain rule as dense products in node order; Welch-Satterthwaite per JCGM 100:2008 Annex G [unverified clause], with the t-quantile computed by a `fid-num`-only inverse.
- **Errors:** `no_derivative` (passed through), `dof_missing{input}`, `invalid_covariance` (from DIST).
- **Determinism:** D0.
- **Performance budget:** model with 30 000 inputs and 20 operators in under 200 ms [unverified until measured].
- **Crate:** `fid-kunc` (module `prop::lpu`; KNP-1 applies).

## Test fixtures

`fixtures/unc/lpu/comb/`: hand-built linear models with closed-form answers, and a Welch-Satterthwaite example with independently computed expectations (CC-BY-4.0).

## Build prompt

> In crate `fid-kunc`, module `prop::lpu`, implement `propagate_lpu(model, inputs, profile) -> Result<LpuResult, LpuError>`. Evaluate the validated model graph once at the input estimates through the kernel `evaluate` interface; for each operator node, in topological order with node-ID tie-break, take its Jacobian from `TangentTable` and chain into output-to-input sensitivities. Build the input covariance from distribution standard uncertainties and declared correlations; compute u_c^2 = c^T V c with fixed-order reductions. Profile `lpu-profile@1`: fixed k (default 2), or Welch-Satterthwaite (`dof_missing` if a type-A input lacks degrees of freedom). Set `lpu_not_applicable` when a kink margin is below `kink_factor` (default 3) times the propagated standard uncertainty of the kinked quantity. Encode the result canonically. Tests AC-1..AC-5 with `fixtures/unc/lpu/comb/`. Constraints: KNP-1, no finite differences, CONSTITUTION 8.

## Open questions

- The defaults (k = 2, kink_factor = 3) are for review by an uncertainty expert [unverified].

## Children

_Leaf: none._
