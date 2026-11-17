---
id: UNC.PROP.LPU.TAN
title: Operator tangent contract, table and coverage report
level: 4
parent: UNC.PROP.LPU
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.PROP.LPU-001]
covers: []
depends_on: [KRN.MAN.KCI, KRN.MAN.CARD, UNC.MODEL.GRAPH]
uses_artifacts: [KRN.MAN.MANI]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.PROP.LPU.TAN: Operator tangent contract, table and coverage report

## Purpose

The single contract through which LPU obtains derivatives of kernel operators. Each operator in an Uncertainty Model is identified by its exact algorithm ID and semver; a tangent provider registered for exactly that ID returns the operator's Jacobian with respect to its numeric inputs at the evaluation point. If no provider exists, LPU fails with `no_derivative` and never falls back to finite differences. A coverage report, run in CI against the published kernel manifest, lists every operator that lacks a tangent.

## Scope

**In:**
- `OperatorTangent` trait and `TangentTable` (ordered map keyed by `alg_id@semver`)
- Jacobian layout convention: rows = output scalar components in the operator's canonical output order, columns = input scalar components in canonical input order
- Optional `Kink { input_component, margin }` hints returned by non-smooth providers
- `no_derivative{operator}` error
- Coverage report `tangent-coverage@1` over the kernel manifest (artifact)
- A test harness that checks any provider against a central-difference oracle in tests only

**Out:**
- Tangents for KRN constructions and evaluation operators (owned by `UNC.PROP.LPU.TCONS`)
- Tangents for the formula IR (owned by `UNC.PROP.LPU.TFORM`)
- Fit sensitivities (owned by `UNC.PROP.LPU.IMPL` and `UNC.PROP.LPU.ASET`)
- Accumulation over the model graph (owned by `UNC.PROP.LPU.COMB`)

## Requirements

- **REQ-UNC.PROP.LPU.TAN-001** The table shall resolve a provider only by exact algorithm ID and semver, and if none is registered, then lookup shall return `no_derivative` naming the operator ID.
- **REQ-UNC.PROP.LPU.TAN-002** Every provider shall return a dense Jacobian whose shape equals (output component count, input component count) of the operator call, and if the shape differs, then the table shall return `tangent_shape_mismatch` naming the operator.
- **REQ-UNC.PROP.LPU.TAN-003** The engine-facing API shall contain no finite-difference code path, verified by a lint test that fails if any function in the LPU modules evaluates an operator at a perturbed input.
- **REQ-UNC.PROP.LPU.TAN-004** When the coverage report runs on a kernel manifest, it shall list, in algorithm-ID order, every operator with no registered provider and every provider whose operator is absent from the manifest.
- **REQ-UNC.PROP.LPU.TAN-005** The test harness shall accept a provider when every Jacobian entry agrees with a central-difference oracle to a relative 1e-6 [unverified] on the provider's registered sample points.

## Acceptance criteria

- **AC-1** Given a test table with providers for `test.affine.map@1.0.0` and a model fixture `fixtures/unc/lpu/tan/affine_model.json`, when the Jacobian is requested, then it equals the affine matrix bit for bit.
- **AC-2** (error path) Given the same model referencing `test.affine.map@1.1.0`, when looked up, then the result is `no_derivative{operator: "test.affine.map@1.1.0"}`.
- **AC-3** (error path) Given a provider returning a 2x3 matrix for a 3x3 operator, then `tangent_shape_mismatch`.
- **AC-4** Given manifest fixture `fixtures/unc/lpu/tan/manifest_min.json` (5 operators, 3 with providers, 1 orphan provider), when the coverage report runs, then it lists the 2 missing and 1 orphan in ID order.
- **AC-5** Given the harness and a provider with a deliberately wrong sign in one entry, then the harness rejects it naming the entry.

## Design

- **Registry command(s):** none (library).
- **Data structures:** `trait OperatorTangent { fn alg_id(&self) -> &AlgId; fn jacobian(&self, params: &[u8], inputs: &[ScalarView]) -> Result<TangentOut, TanError>; }`; `TangentOut { jac: Mat, kinks: Vec<Kink> }`; `TangentTable(BTreeMap<AlgId, Box<dyn OperatorTangent>>)`; `CoverageReport { missing: Vec<AlgId>, orphan: Vec<AlgId>, version: "tangent-coverage@1" }`.
- **Algorithm:** lookup by exact key; shape check; the report is a sorted set difference.
- **Errors:** `no_derivative{operator}`, `tangent_shape_mismatch{operator}`, provider errors passed through typed.
- **Determinism:** D0.
- **Performance budget:** lookup under 1 us [unverified until measured].
- **Crate:** `fid-kunc` (module `prop::lpu::tan`; KNP-1 applies).

## Test fixtures

`fixtures/unc/lpu/tan/`: hand-written affine model and a minimal manifest (CC-BY-4.0). The CI coverage job reads the published kernel manifest artifact (`KRN.MAN.MANI`).

## Build prompt

> In crate `fid-kunc`, module `prop::lpu::tan`, implement the operator tangent contract: trait `OperatorTangent` (exact `alg_id@semver`, `jacobian(params, inputs) -> TangentOut { jac, kinks }`), `TangentTable` as a BTreeMap keyed by exact algorithm ID with semver, errors `no_derivative{operator}` and `tangent_shape_mismatch{operator}`. Jacobian rows are output components in canonical output order, columns input components in canonical input order. Provide `coverage_report(manifest) -> CoverageReport` (`tangent-coverage@1`: operators without providers and orphan providers, sorted). Provide a test-only harness comparing a provider with a central-difference oracle (relative 1e-6). The engine code must contain no finite-difference path; add a lint test for it. Tests AC-1..AC-5 with `fixtures/unc/lpu/tan/`. Constraints: KNP-1 (no FMA, fid-num only, no HashMap, NaN is an error), CONSTITUTION 8 (no panic, no unwrap, no unsafe).

## Open questions

- The oracle tolerance 1e-6 suits double-precision central differences on well-scaled operators; badly scaled operators may need per-provider scaling [unverified].

## Children

_Leaf: none._
