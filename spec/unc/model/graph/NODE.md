---
id: UNC.MODEL.GRAPH
title: Model graph schema, digest, validation and versioning
level: 3
parent: UNC.MODEL
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.MODEL-001, REQ-UNC.MODEL-003, REQ-UNC.MODEL-004]
covers: []
depends_on: [UNC.MODEL.DIST, KRN]
cli: [fid unc create-model, fid unc validate-model, fid unc show-model]
mcp: [unc_create_model, unc_validate_model, unc_show_model]
journal: [unc.create_model@1]
updated: 2026-11-16
---

# UNC.MODEL.GRAPH: Model graph schema, digest, validation and versioning

## Purpose

Defines the Uncertainty Model (`um:<name>@<semver>`) as a content-addressed directed acyclic graph of input quantities, kernel-operator nodes and output quantities, in the style of JCGM GUM-6:2020. It owns the canonical form and digest, the structural validation that must pass before any engine evaluates a model, and the version rule that a changed model is a new version while old results keep pointing at the old one.

## Scope

**In:**
- Graph schema: input nodes (distribution from `UNC.MODEL.DIST`, source reference), operator nodes (kernel algorithm ID and version from the kernel manifest), output nodes (quantity kind, unit, characteristic link), edges, correlation groups
- Canonical form (RFC 8785, sorted nodes and edges) and SHA-256 digest
- Validation: acyclicity, operator membership in the kernel manifest, arity and unit consistency on edges, every input sourced, correlation matrices valid (check delegated to DIST)
- Versioning: semver bump rule, immutability of published versions, results pinned to version and digest
- Commands `unc.create_model@1` (W2), `unc.validate_model@1` (R1), `unc.show_model@1` (R0)

**Out:**
- Distribution types and the correlation check (owned by `UNC.MODEL.DIST`)
- Contributor sub-models and their library (owned by `UNC.MODEL.CONTRIB`)
- Binding to plans and adoption (owned by `UNC.MODEL.BIND`)
- Evaluation (owned by `UNC.PROP`)

## Requirements

- **REQ-UNC.MODEL.GRAPH-001** The system shall identify every model by `um:<name>@<semver>` and by the SHA-256 digest of its RFC 8785 canonical form, in which nodes are sorted by node ID and edges by (target, port).
- **REQ-UNC.MODEL.GRAPH-002** If a model graph contains a cycle, then validation shall fail with error `cycle` naming one node on the cycle.
- **REQ-UNC.MODEL.GRAPH-003** If an operator node names an algorithm ID or version not present in the current kernel manifest, then validation shall fail with error `unknown_operator`.
- **REQ-UNC.MODEL.GRAPH-004** If an input node has no source reference, then validation shall fail with error `unsourced_input`.
- **REQ-UNC.MODEL.GRAPH-005** If an edge connects ports of incompatible quantity kind or unit, then validation shall fail with error `unit_mismatch`.
- **REQ-UNC.MODEL.GRAPH-006** If a model is submitted under an existing `um:<name>@<semver>` with a different digest, then `unc.create_model@1` shall fail with error `version_conflict` and commit nothing.
- **REQ-UNC.MODEL.GRAPH-007** When a result is computed under a model, the result shall reference the model's name, semver and digest, and later versions shall never rewrite that reference.
- **REQ-UNC.MODEL.GRAPH-008** Validation of a model with 1,000 nodes shall complete in under 50 ms on reference hardware [unverified].

## Interfaces

- **Offers:** `UncertaintyModel` type; `validate(model, manifest) -> Result<ValidModel, ModelError>` (only `ValidModel` is accepted by `UNC.PROP`); commands `unc.create_model@1`, `unc.validate_model@1`, `unc.show_model@1`.
- **Consumes:** distribution and correlation validation (UNC.MODEL.DIST); kernel manifest algorithm list (KRN.MAN); `ModelRef` type (UNC.UEC.STMT).

## Decomposition hints

SCHEMA (types, canonical form, digest), CHECK (validation passes), STORE (create/show commands and version immutability).

## Open questions

- How a model references a kernel pipeline that is itself parameterised (for example a fit with an outlier policy) is not settled: either the operator node pins the full parameter digest, or parameters are model inputs. Decide when splitting.

## Children

_None yet: status `open`._
