---
id: UNC.MODEL.GRAPH.CHECK
title: Structural validation passes
level: 4
parent: UNC.MODEL.GRAPH
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.MODEL.GRAPH-002, REQ-UNC.MODEL.GRAPH-003, REQ-UNC.MODEL.GRAPH-004, REQ-UNC.MODEL.GRAPH-005, REQ-UNC.MODEL.GRAPH-008]
covers: []
depends_on: [UNC.MODEL.GRAPH.SCHEMA, UNC.MODEL.DIST, KRN.MAN.MANI]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.MODEL.GRAPH.CHECK: Structural validation passes

## Purpose

`validate(model, manifest) -> Result<ValidModel, Vec<ModelError>>`: the gate every model passes before an engine evaluates it. Passes run in a fixed order and report every finding, not only the first: acyclicity, operator membership in the kernel manifest, port arity, unit and quantity-kind consistency on edges, sourced inputs, and correlation matrices (delegated to `UNC.MODEL.DIST`). Only a `ValidModel` is accepted by `UNC.PROP`.

## Scope

**In:** the passes above; `ValidModel` newtype that cannot be built other than by `validate`; finding ordering.
**Out:** types and digest (owned by `UNC.MODEL.GRAPH.SCHEMA`); correlation-matrix mathematics (owned by `UNC.MODEL.DIST`); the manifest (owned by `KRN.MAN.MANI`).

## Requirements

- **REQ-UNC.MODEL.GRAPH.CHECK-001** If the graph has a cycle, then validation shall report `cycle` naming the lowest node ID on the cycle.
- **REQ-UNC.MODEL.GRAPH.CHECK-002** If an operator's algorithm ID and version are not in the supplied kernel manifest, then validation shall report `unknown_operator` naming the node.
- **REQ-UNC.MODEL.GRAPH.CHECK-003** If an input node has no source reference or a source with an empty digest, then validation shall report `unsourced_input`.
- **REQ-UNC.MODEL.GRAPH.CHECK-004** If an edge connects ports of different quantity kind or unit dimension, or a port's connection count differs from its declared arity, then validation shall report `unit_mismatch` or `arity_mismatch`.
- **REQ-UNC.MODEL.GRAPH.CHECK-005** If a correlation group's matrix fails `UNC.MODEL.DIST` validation, then validation shall report `invalid_correlation` naming the group.
- **REQ-UNC.MODEL.GRAPH.CHECK-006** Validation shall report all findings sorted by (pass order, node ID), and shall complete for a 1 000-node model in under 50 ms on reference hardware [unverified].

## Acceptance criteria

- **AC-1** Given `fixtures/unc/model/graph/bore_ls.json` and manifest stub `manifest_stub.json`, then validation returns `ValidModel`.
- **AC-2** (error paths) Given `fixtures/unc/model/graph/bad/` (cycle among n3, n5, n7; unknown operator; unsourced input; mm-to-rad edge; indefinite correlation), then each yields its code with the named node, and a file with all five yields five findings in the stated order.
- **AC-3** (performance) Given a generated 1 000-node chain model (seed `0x47524150`), then validation completes within budget on the reference runner.

## Design

- **Registry command(s):** none (called by `unc.validate_model@1` in `UNC.MODEL.GRAPH.STORE`).
- **Algorithm:** Kahn topological sort with BTreeMap-ordered ready set; manifest lookup; unit dimension vectors compared exactly; correlation via `dist::validate_correlation`.
- **Errors:** findings `cycle`, `unknown_operator`, `unsourced_input`, `unit_mismatch`, `arity_mismatch`, `invalid_correlation`.
- **Determinism:** D0.
- **Performance budget:** REQ-006.
- **Crate:** `fid-kunc` (module `model`).

## Test fixtures

`fixtures/unc/model/graph/` plus `bad/` variants and a manifest stub (CC-BY-4.0).

## Build prompt

> In crate `fid-kunc`, module `model` (kernel; KNP-1; no panic or `unwrap`; BTreeMap only), implement `validate(model: &UncertaintyModel, manifest: &KernelManifest) -> Result<ValidModel, Vec<ModelError>>`. Passes in order: acyclicity (Kahn, report `cycle` with lowest node ID on the cycle), operator membership (`unknown_operator`), sourced inputs (`unsourced_input`), edge unit/kind and port arity (`unit_mismatch`, `arity_mismatch`), correlation groups via `dist::validate_correlation` (`invalid_correlation`). Report all findings sorted by (pass, node ID). `ValidModel` constructible only here. Tests AC-1..AC-3 with `fixtures/unc/model/graph/`; 1 000-node model under 50 ms.

## Open questions

- None.

## Children

_Leaf: none._
