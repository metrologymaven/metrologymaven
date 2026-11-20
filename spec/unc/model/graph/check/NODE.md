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
built_in: [fid-kunc]
satisfies: [REQ-UNC.MODEL.GRAPH-002, REQ-UNC.MODEL.GRAPH-003, REQ-UNC.MODEL.GRAPH-004, REQ-UNC.MODEL.GRAPH-005, REQ-UNC.MODEL.GRAPH-008]
covers: []
depends_on: [UNC.MODEL.GRAPH.SCHEMA, UNC.MODEL.DIST, KRN.MAN.MANI]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-20
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

## Resolved at build

- **Passes.** Besides the five named passes, validation fails closed on three more findings, numbered into the pinned order: `dangling_edge` (pass 1: an edge end whose node or port does not exist, or that points the wrong way, out of an output node or into an input node), `unreachable_output` (pass 6: no input reaches the output over resolved edges) and `correlation_not_input` (pass 7: a group member that is not an input node). The order is dangling_edge, cycle, unknown_operator, unsourced_input, arity_mismatch and unit_mismatch, unreachable_output, correlation_not_input, invalid_correlation; the five findings of AC-2 keep their stated relative order.
- **Resolution.** A source is an input's implicit port `value` or an operator output port; a target is an operator input port or an output's implicit `value` (arity 1). Arity counts every edge whose target resolves, even if its source dangles, so one broken edge is not also reported as a missing connection. Cycles, unit checks and reachability use only edges with both ends resolved.
- **Cycles.** After Kahn leaves nodes, each strongly connected component of the remainder that is a cycle (two or more nodes, or a self-loop) is one `cycle` finding naming its lowest node ID and listing its members (iterative Tarjan, linear time, no recursion).
- **Units** are compared as unit codes exactly. `si:rad` and `si:1` share a dimension vector but are different codes, so an angle cannot flow silently into a ratio port; quantity kinds must also be equal. Both give `unit_mismatch`.
- **Empty digest** (REQ-003): a source digest is empty when it is the tombstone digest (64 zeros) or the SHA-256 of zero bytes.
- **Sort key** (REQ-006): pass, node ID, then the remaining fields, all as bytes (arity numbers by value; on one port `arity_mismatch` sorts before `unit_mismatch`). The node ID of an edge finding is its target node; of a correlation finding, the group's smallest input ID.
- **Manifest.** Membership is exact `algorithm_id@algorithm_version` in `KernelManifest::core().algorithms` (`KRN.MAN.MANI.SCHEMA`, in `fid-kci`). Each entry carries its `KRN.MAN.CARD` digest, so a listed operator also has a registered card. `KRN.MAN.MANI.GEN`, which produces the real manifest, is not needed here: the caller (`UNC.MODEL.GRAPH.STORE`) supplies the manifest.
- **`ValidModel`** has private fields and no constructor other than `validate`; it keeps the model, the digest of the manifest it was checked against, and the Kahn order (smallest ready ID first).
- **Error type.** Findings are `fid_kunc::model::ModelError` (one line each via `Display`, `<code> <node> ...`). The parent's `Result<ValidModel, ModelError>` is read as `Result<ValidModel, Vec<ModelError>>`, as this leaf's Purpose states.

## Children

_Leaf: none._
