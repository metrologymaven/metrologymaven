---
id: UNC.MODEL.CONTRIB.CONTRACT
title: Contributor contract, expansion and read commands
level: 4
parent: UNC.MODEL.CONTRIB
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.MODEL.CONTRIB-001, REQ-UNC.MODEL.CONTRIB-002, REQ-UNC.MODEL.CONTRIB-006]
covers: []
depends_on: [UNC.MODEL.GRAPH.SCHEMA, UNC.MODEL.GRAPH.CHECK, UNC.MODEL.DIST]
side_effect: [R0, R0]
determinism: D0
cli: [fid unc list-contributor, fid unc show-contributor]
mcp: [unc_list_contributor, unc_show_contributor]
journal: [unc.list_contributor@1, unc.show_contributor@1]
built_in: [fid-unc]
updated: 2026-11-22
---

# UNC.MODEL.CONTRIB.CONTRACT: Contributor contract, expansion and read commands

## Purpose

Defines `contributor/v1`, the data-only contract every contributor follows (ID `uc:<name>@<semver>`, parameters with units, valid ranges and required source kinds, a sub-graph template, applicability limits, references by designator and clause), the `expand` function that instantiates a contributor into a validated sub-graph, and the two read commands over the shipped library.

## Scope

**In:** `contributor/v1` schema and digest; `expand(contributor, params, sources) -> SubGraph`; parameter range and source-kind checks; validation of the expanded sub-graph through `UNC.MODEL.GRAPH.CHECK`; loading the shipped library; `unc.list_contributor@1`, `unc.show_contributor@1` (R0).
**Out:** individual contributors (owned by `UNC.MODEL.CONTRIB.THERM`, `.STYLUS`, `.SAMPLE`, `.REG`); instrument contributors (owned by `UNC.INST`).

## Requirements

- **REQ-UNC.MODEL.CONTRIB.CONTRACT-001** Every contributor file shall declare `uc:<name>@<semver>`, parameters with unit, valid range and required source kinds, a sub-graph template whose operators are kernel algorithm IDs, and its digest over RFC 8785 canonical JSON.
- **REQ-UNC.MODEL.CONTRIB.CONTRACT-002** When `expand` is called, the system shall substitute parameters, attach the supplied source references, prefix node IDs with the instance ID, and validate the result with `UNC.MODEL.GRAPH.CHECK`, failing with `contributor_invalid` carrying the findings if validation fails.
- **REQ-UNC.MODEL.CONTRIB.CONTRACT-003** If a parameter lies outside its declared valid range, then `expand` shall fail with `out_of_range` naming the parameter and bound and shall not extrapolate.
- **REQ-UNC.MODEL.CONTRIB.CONTRACT-004** If a parameter's source reference is missing or of a kind the contributor does not accept, then `expand` shall fail with `source_kind_not_accepted`.
- **REQ-UNC.MODEL.CONTRIB.CONTRACT-005** When `unc.list_contributor@1` is called, the system shall return ID, semver, digest, parameter names and applicability of every shipped contributor sorted by ID; `unc.show_contributor@1` shall return one contributor in full.

## Acceptance criteria

- **AC-1** Given `fixtures/unc/model/contrib/uc_example.json` (an invented two-parameter contributor) and valid parameters, when `expand` runs, then the sub-graph validates, node IDs carry the instance prefix and the digest equals `expected_expand.sha256`.
- **AC-2** (error paths) parameter above its range → `out_of_range`; `declared` source where only `certificate` is accepted → `source_kind_not_accepted`; template with an unknown operator → `contributor_invalid` with `unknown_operator`.
- **AC-3** Given the shipped library stub, when `fid unc list-contributor --json` runs, then entries are sorted by ID with the listed fields.

## Design

- **Registry command(s):** `unc.list_contributor@1` (R0; filter `applicability`), `unc.show_contributor@1` (R0; `id`).
- **Data structures:** `ContributorModel { id, semver, params: Vec<ParamSpec{name, unit, min, max, source_kinds}>, template: UncertaintyModel, applicability, references: Vec<StandardRef> }`; `SubGraph`.
- **Errors:** `out_of_range`, `source_kind_not_accepted`, `contributor_invalid{findings}`, `contributor_not_found`.
- **Determinism:** D0.
- **Performance budget:** expand under 1 ms per contributor [unverified until measured].
- **Crate:** `fid-unc` (module `contrib`).

## Test fixtures

`fixtures/unc/model/contrib/`: invented contributors and a library stub (CC-BY-4.0).

## Build prompt

> In crate `fid-unc`, module `contrib`, define `contributor/v1` (`uc:<name>@<semver>`, parameter specs with unit, valid range and accepted source kinds, a sub-graph template using `fid-kunc` model types, applicability, references by designator and clause only) and `expand(contributor, params, sources) -> Result<SubGraph, ContribError>`: range check (`out_of_range`, never extrapolate), source kind check (`source_kind_not_accepted`), instance-prefixed node IDs, validation via `fid-kunc` `model::validate` (`contributor_invalid{findings}`). Registry commands `unc.list_contributor@1`, `unc.show_contributor@1` (R0; `--json`). Tests AC-1..AC-3 with `fixtures/unc/model/contrib/`. No panic or `unwrap`; BTreeMap only.

## Open questions

- Whether third-party contributors are data only (assumed here) or code plug-ins (A0 install) should follow ECO.SDK.

## Children

_Leaf: none._
