---
id: UNC.MODEL.GRAPH.SCHEMA
title: Model graph types, canonical form and digest
level: 4
parent: UNC.MODEL.GRAPH
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-kunc]
satisfies: [REQ-UNC.MODEL.GRAPH-001]
covers: []
depends_on: [UNC.MODEL.DIST, UNC.UEC.STMT, CORE.JRN.CANON]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.MODEL.GRAPH.SCHEMA: Model graph types, canonical form and digest

## Purpose

The data types of an Uncertainty Model (`um:<name>@<semver>`): input nodes, operator nodes, output nodes, edges and correlation groups, the `uncertainty-model/v1` JSON schema, the canonical form and the SHA-256 digest. Validation and storage build on these types.

## Scope

**In:**
- `InputNode { id, quantity_kind, unit, distribution, source: SourceRef }`; `OperatorNode { id, algorithm_id, algorithm_version, params_digest, ports_in, ports_out }`; `OutputNode { id, quantity_kind, unit, characteristic: Option<CharId> }`; `Edge { from: (node, port), to: (node, port) }`; `CorrelationGroup { inputs, matrix }`; `SourceRef { kind: certificate | dcc | qualification | type_a_dataset | declared, digest, author }`
- Canonical form: nodes sorted by ID, edges by (target node, target port), correlation groups by first input ID; RFC 8785 serialisation; SHA-256 digest
- `ModelRef { name, semver, digest }` construction (type from `UNC.UEC.STMT`)

**Out:**
- Validation (owned by `UNC.MODEL.GRAPH.CHECK`); commands and storage (owned by `UNC.MODEL.GRAPH.STORE`); distributions (owned by `UNC.MODEL.DIST`)

## Requirements

- **REQ-UNC.MODEL.GRAPH.SCHEMA-001** The model shall be identified by `um:<name>@<semver>`, where name matches `[a-z0-9][a-z0-9-]{0,63}` and semver follows Semantic Versioning 2.0.0.
- **REQ-UNC.MODEL.GRAPH.SCHEMA-002** The digest shall be SHA-256 over RFC 8785 canonical JSON of the model with nodes sorted by node ID and edges by (target node, target port), so that any permutation of input order yields the same digest.
- **REQ-UNC.MODEL.GRAPH.SCHEMA-003** Every operator node shall carry the digest of its full parameter set, and a change to any parameter shall change the model digest.
- **REQ-UNC.MODEL.GRAPH.SCHEMA-004** If a model file fails the JSON schema, exceeds 16 MB or nesting depth 32, or has duplicate node IDs, then parsing shall fail with `schema_invalid{field}`, `too_large` or `duplicate_node`.

## Acceptance criteria

- **AC-1** Given `fixtures/unc/model/graph/bore_ls.json`, when parsed and digested, then the digest equals `bore_ls.sha256` on wasm32, x86-64 and ARM64.
- **AC-2** Given `bore_ls_shuffled.json` (same content, nodes and edges permuted), then the digest is identical to AC-1.
- **AC-3** Given `bore_ls_param.json` (one operator parameter changed), then the digest differs.
- **AC-4** (error paths) Given `dup_node.json`, `bad_name.json` and a 17 MB file, then `duplicate_node`, `schema_invalid{name}` and `too_large`.

## Design

- **Registry command(s):** none.
- **Data structures:** as in Scope; `UncertaintyModel { name, semver, inputs, operators, outputs, edges, correlations }`.
- **Algorithm:** bounded serde parse with `deny_unknown_fields`; sort; canonical JSON via `CORE.JRN.CANON`; SHA-256.
- **Errors:** `schema_invalid{field}`, `too_large`, `duplicate_node`.
- **Determinism:** D0.
- **Performance budget:** digest of a 1 000-node model under 5 ms [unverified until measured].
- **Crate:** `fid-kunc` (module `model`).

## Test fixtures

`fixtures/unc/model/graph/`: project-authored models (bore diameter by LS circle fit with thermal input) and variants (CC-BY-4.0).

## Build prompt

> In crate `fid-kunc`, module `model` (kernel; no I/O; no panic or `unwrap`; BTreeMap only), define `UncertaintyModel` and the `uncertainty-model/v1` JSON schema: input nodes (quantity kind, unit, `Distribution` from module `dist`, `SourceRef {kind: certificate|dcc|qualification|type_a_dataset|declared, digest, author}`), operator nodes (algorithm ID, version, params digest, ports), output nodes (quantity kind, unit, optional characteristic), edges and correlation groups. Name `[a-z0-9][a-z0-9-]{0,63}`, SemVer 2.0.0. Canonical form: nodes sorted by ID, edges by (target, port); RFC 8785 via the canonical JSON crate; SHA-256 digest; `ModelRef {name, semver, digest}`. Bounded parse (16 MB, depth 32): `schema_invalid{field}`, `too_large`, `duplicate_node`. Tests AC-1..AC-4 with `fixtures/unc/model/graph/`.

## Open questions

- None.

## Children

_Leaf: none._
