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
built_pr: 310
satisfies: [REQ-UNC.MODEL.GRAPH-001]
covers: []
depends_on: [UNC.MODEL.DIST, UNC.UEC.STMT, CORE.JRN.CANON]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-20
---

# UNC.MODEL.GRAPH.SCHEMA: Model graph types, canonical form and digest

## Purpose

The data types of an Uncertainty Model (`um:<name>@<semver>`): input nodes, operator nodes, output nodes, edges and correlation groups, the `uncertainty-model/v1` JSON schema, the canonical form and the SHA-256 digest. Validation and storage build on these types.

## Scope

**In:**
- `InputNode { id, quantity_kind, unit, distribution, source: SourceRef }`; `OperatorNode { id, algorithm_id, algorithm_version, params, ports_in, ports_out }` with the full parameter set `params` embedded and `params_digest` derived from it; `OutputNode { id, quantity_kind, unit, characteristic: Option<CharId> }`; `Edge { from: (node, port), to: (node, port) }`; `CorrelationGroup { inputs, matrix }`; `SourceRef { kind: certificate | dcc | qualification | type_a_dataset | declared, digest, author }`
- Canonical form: nodes sorted by ID, edges by (target node, target port), correlation groups by first input ID; RFC 8785 serialisation; SHA-256 digest
- `ModelRef { name, semver, digest }` construction (type from `UNC.UEC.STMT`)

**Out:**
- Validation (owned by `UNC.MODEL.GRAPH.CHECK`); commands and storage (owned by `UNC.MODEL.GRAPH.STORE`); distributions (owned by `UNC.MODEL.DIST`)

## Requirements

- **REQ-UNC.MODEL.GRAPH.SCHEMA-001** The model shall be identified by `um:<name>@<semver>`, where name matches `[a-z0-9][a-z0-9-]{0,63}` and semver follows Semantic Versioning 2.0.0.
- **REQ-UNC.MODEL.GRAPH.SCHEMA-002** The digest shall be SHA-256 over RFC 8785 canonical JSON of the model with nodes sorted by node ID and edges by (target node, target port), so that any permutation of input order yields the same digest.
- **REQ-UNC.MODEL.GRAPH.SCHEMA-003** Every operator node shall embed its full parameter set and carry its digest, derived as SHA-256 over the parameters' RFC 8785 canonical JSON (never supplied independently; a supplied digest that differs from the recomputed one fails `schema_invalid{operators.params_digest}`), and a change to any parameter shall change the model digest.
- **REQ-UNC.MODEL.GRAPH.SCHEMA-004** If a model file fails the JSON schema, exceeds 16 MB, nesting depth 32 or 2^20 JSON values, or has duplicate node IDs, then parsing shall fail with `schema_invalid{field}`, `too_large` or `duplicate_node`.

## Acceptance criteria

- **AC-1** Given `fixtures/unc/model/graph/bore_ls.json`, when parsed and digested, then the digest equals `bore_ls.sha256` on wasm32, x86-64 and ARM64.
- **AC-2** Given `bore_ls_shuffled.json` (same content, nodes and edges permuted), then the digest is identical to AC-1.
- **AC-3** Given `bore_ls_param.json` (one embedded operator parameter changed), then the operator's derived `params_digest` and the model digest differ; given a document whose supplied `params_digest` does not match its `params`, then `schema_invalid{operators.params_digest}`.
- **AC-4** (error paths) Given `dup_node.json`, `bad_name.json` and a 17 MB file, then `duplicate_node`, `schema_invalid{name}` and `too_large`.

## Design

- **Registry command(s):** none.
- **Data structures:** as in Scope; `UncertaintyModel { name, semver, inputs, operators, outputs, edges, correlations }`.
- **Algorithm:** bounded parse (16 MiB, depth 32, 2^20 values) refusing unknown members; sort; canonical JSON via the kernel RFC 8785 writer (`fid_kci::kci::json`, kernel layer, drift-guarded against `CORE.JRN.CANON` by `crates/fid-canon/tests/kci_drift.rs`); SHA-256.
- **Errors:** `schema_invalid{field}`, `too_large`, `duplicate_node`.
- **Determinism:** D0.
- **Performance budget:** digest of a 1 000-node model under 5 ms [unverified until measured].
- **Crate:** `fid-kunc` (module `model`).

## Test fixtures

`fixtures/unc/model/graph/`: project-authored models (bore diameter by LS circle fit with thermal input) and variants (CC-BY-4.0).

## Build prompt

> In crate `fid-kunc`, module `model` (kernel; no I/O; no panic or `unwrap`; BTreeMap only), define `UncertaintyModel` and the `uncertainty-model/v1` JSON schema: input nodes (quantity kind, unit, `Distribution` from module `dist`, `SourceRef {kind: certificate|dcc|qualification|type_a_dataset|declared, digest, author}`), operator nodes (algorithm ID, version, embedded params with a derived params digest, ports), output nodes (quantity kind, unit, optional characteristic), edges and correlation groups. Name `[a-z0-9][a-z0-9-]{0,63}`, SemVer 2.0.0. Canonical form: nodes sorted by ID, edges by (target, port); RFC 8785 via the canonical JSON crate; SHA-256 digest; `ModelRef {name, semver, digest}`. Bounded parse (16 MB, depth 32): `schema_invalid{field}`, `too_large`, `duplicate_node`. Tests AC-1..AC-4 with `fixtures/unc/model/graph/`.

## Open questions

- Units: the kernel unit set is `si:m`, `si:rad` and `si:1` (CONSTITUTION §4). The thermal contributor (`UNC.MODEL.CONTRIB.THERM`) needs `si:K` and `si:K-1`, or units as dimension vectors. Decide before THERM or CONTRIB are built. Adding unit codes later does not change the digest of any existing model, so the set is not widened now.

## Resolved at build

- **semver** is the core `MAJOR.MINOR.PATCH` subset of Semantic Versioning 2.0.0 (each part 1 to 9 digits, no leading zero). Pre-release and build metadata are refused: build metadata gives equal precedence to different strings, and the `UNC.UEC.STMT` `ModelRef` accepts only the core form. One `is_semver` serves both.
- **Text** is printable ASCII only, with a pattern per field (module documentation of `fid_kunc::model`). The reader refuses escapes and every other byte, so bidi controls, control characters and non-NFC text cannot occur without Unicode tables in a kernel crate.
- **Ports.** Operator input ports are `{name, quantity_kind, unit, arity}`, output ports `{name, quantity_kind, unit}`. Port lists keep their declared order, which is the operator's argument and result order, and are not sorted. Repeated port names are refused.
- **Implicit ports.** An input node has exactly one output port and an output node exactly one input port, both named `value`; edges name them like any other port.
- **Edge order tie-break.** Edges sort by (target node, target port, source node, source port), so several edges into one port have a total order. Repeated edges are refused.
- **`source` is optional** on an input node so that `UNC.MODEL.GRAPH.CHECK` can report `unsourced_input`; when present its digest is a valid `sha256:` digest.
- **Operator parameters** are embedded (`params`, an object within the reader rules); `params_digest` is derived and always written in the canonical form. Model documents are self-contained and replayable.
- **Error order.** Repeated node IDs and inputs repeated within or across correlation groups are refused before any other check on the parts, so every sort is total and the first error from the parts does not depend on the order of nodes, edges or groups. Parse-stage errors (types, encodings, unknown members) follow document order.

## Children

_Leaf: none._
