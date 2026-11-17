---
id: UNC.MODEL
title: Measurement models and contributor library
level: 2
parent: UNC
kind: subsystem
status: split
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-UNC-002]
covers: []
depends_on: [UNC.UEC, KRN]
cli: [fid unc *-model, fid unc *-contributor, fid unc run-substitution]
mcp: [unc_*_model, unc_*_contributor, unc_run_substitution]
journal: [unc.*_model, unc.run_substitution]
updated: 2026-11-16
---

# UNC.MODEL: Measurement models and contributor library

## Purpose

Owns the **Uncertainty Model** (`um:<name>@x.y.z`): an explicit, content-addressed measurement model graph in the style of JCGM GUM-6:2020, whose inputs carry probability distributions, correlations and source references, whose function nodes are kernel operators, and whose outputs are characteristics. It also owns the library of non-instrument contributor models (thermal, sampling and unsampled form, stylus qualification, scan registration) and the ISO 15530-3 substitution workflow's input structure. It is separate because the model is the reviewable, attestable object; propagation engines only evaluate it.

## Scope

**In:**
- The model graph schema, versioning and content digest
- Input distributions: normal, rectangular, triangular, arcsine, scaled-t, trapezoidal, multivariate normal; correlation matrices
- Source references on inputs (calibration certificate or DCC, qualification record, type-A dataset, declared value with author)
- Contributor model plug-in contract and the non-instrument contributor library
- Model templates and binding a model to a characteristic in a plan
- ISO 15530-3 substitution: inputs (calibrated workpiece value and U, repeated runs, production runs) and the component breakdown
- Agent-drafted models as proposals requiring human adoption

**Out** (and which node owns it instead):
- Evaluating a model by LPU or Monte Carlo (owned by `UNC.PROP`)
- Instrument error models, MPE-derived models and IEMPS (owned by `UNC.INST`)
- Transform and network-level models (owned by `UNC.XFM`)
- Result fields and class assignment (owned by `UNC.UEC`)

## Requirements

- **REQ-UNC.MODEL-001** The system shall represent every uncertainty model as a directed acyclic graph of input quantities, kernel operator nodes and output quantities, identified by `um:<name>@<semver>` and a SHA-256 digest of its canonical form.
- **REQ-UNC.MODEL-002** Every input quantity shall declare a distribution from the supported set, its parameters with units, and a source reference.
- **REQ-UNC.MODEL-003** If a model graph contains a cycle, an operator not in the kernel manifest, an input without a source, or a correlation matrix that is not symmetric positive semi-definite, then the system shall reject it with a typed error and shall not evaluate it.
- **REQ-UNC.MODEL-004** When a model's content changes, the system shall require a new version and digest, and results computed under the previous version shall keep referencing it.
- **REQ-UNC.MODEL-005** The contributor library shall provide versioned models for at least thermal expansion (sensor uncertainty and CTE uncertainty), probe or stylus qualification, sampling and unsampled form, and scan registration [unverified: completeness against ISO 15530 contributor lists].
- **REQ-UNC.MODEL-006** When an ISO 15530-3 substitution evaluation is run, the system shall take the calibrated workpiece value and its uncertainty, at least the minimum number of repeated runs set by the procedure profile, and the production measurements, and shall report each component separately [unverified: component naming and minimum run count against ISO 15530-3:2011].
- **REQ-UNC.MODEL-007** If an agent drafts or edits a model, then the system shall mark the model version as an agent proposal and shall refuse to release results computed under it until a human adopts it.

## Interfaces

- **Offers:** `UncertaintyModel` type and graph schema; contributor plug-in contract; commands such as `unc.create_model@1` (W2), `unc.bind_model@1` (W2), `unc.validate_model@1` (R1), `unc.list_contributor@1` (R0), `unc.run_substitution@1` (R1, result commit W2).
- **Consumes:** kernel operator manifest from KRN; `UncertaintyStatement` from `UNC.UEC`; instrument contributor models from `UNC.INST` (as graph inputs).

## Decomposition hints

GRAPH (schema, digest, validation), DIST (distributions and correlation), CONTRIB (contributor plug-ins and library), BIND (plan binding and adoption), SUBST (ISO 15530-3 workflow).

## Open questions

- ISO 15530-3 component naming and any newer edition must be checked against the purchased text [unverified].

## Split rationale

Split by **the object each child owns**: the graph and its identity (GRAPH), the probability layer shared by both propagation methods (DIST), reusable non-instrument contributor sub-graphs (CONTRIB), the plan binding and adoption record (BIND), and the one standardized workflow that produces a model-free task-specific U (SUBST).

Alternatives considered:
- **Distributions inside GRAPH.** Rejected: LPU and MCM in `UNC.PROP` and Monte Carlo fields in `UNC.XFM` use the distributions directly, without the graph; a separate kernel module keeps one sampling semantics.
- **Templates as their own child.** Rejected: a template is a model version with unbound inputs, so it is covered by GRAPH (schema) and BIND (input mapping).
- **Agent adoption in CORE only.** Kept here as a digest-level adoption record so the release gate has one fact to check; branch promotion remains CORE's.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| UNC.MODEL.GRAPH | Model graph schema, digest, validation and versioning | 001 003 004 | none |
| UNC.MODEL.DIST | Input distributions and correlation (leaf) | 002 003 | none |
| UNC.MODEL.CONTRIB | Contributor plug-in contract and non-instrument contributor library | 005 | none |
| UNC.MODEL.BIND | Model binding to characteristics and human adoption (leaf) | 004 007 | none |
| UNC.MODEL.SUBST | ISO 15530-3 substitution evaluation | 006 | none |
