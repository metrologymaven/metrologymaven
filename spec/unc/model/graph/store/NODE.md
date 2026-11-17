---
id: UNC.MODEL.GRAPH.STORE
title: Create, validate and show commands and version immutability
level: 4
parent: UNC.MODEL.GRAPH
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.MODEL.GRAPH-006, REQ-UNC.MODEL.GRAPH-007]
covers: []
depends_on: [UNC.MODEL.GRAPH.CHECK, UNC.MODEL.GRAPH.SCHEMA, CORE.STO.CAS, KRN.MAN.MANI]
side_effect: [W2, R1, R0]
determinism: D0
cli: [fid unc create-model, fid unc validate-model, fid unc show-model]
mcp: [unc_create_model, unc_validate_model, unc_show_model]
journal: [unc.create_model@1, unc.validate_model@1, unc.show_model@1]
updated: 2026-11-17
---

# UNC.MODEL.GRAPH.STORE: Create, validate and show commands and version immutability

## Purpose

The registry commands around the model graph: `unc.create_model@1` stores a validated model version immutably, `unc.validate_model@1` returns findings without storing, and `unc.show_model@1` returns a stored version by reference or digest. A name and semver, once stored, always mean the same digest, and results keep pointing at the exact version they were computed under.

## Scope

**In:** the three commands; the project model store keyed by (name, semver) and by digest; the author principal kind recorded at creation (consumed by `UNC.MODEL.BIND` for adoption); `resolve(ModelRef) -> ValidModel` for engines.
**Out:** binding and adoption (owned by `UNC.MODEL.BIND`); validation passes (owned by `UNC.MODEL.GRAPH.CHECK`).

## Requirements

- **REQ-UNC.MODEL.GRAPH.STORE-001** When `unc.create_model@1` is given a model that passes validation against the current kernel manifest, the system shall store it under (name, semver) and digest, record the author principal kind, and journal one event with the digest.
- **REQ-UNC.MODEL.GRAPH.STORE-002** If `unc.create_model@1` is given an existing (name, semver) with a different digest, then it shall fail with `version_conflict` and commit nothing; with the same digest it shall succeed without a new store entry (idempotent).
- **REQ-UNC.MODEL.GRAPH.STORE-003** If the model fails validation, then `unc.create_model@1` shall fail with `model_invalid` carrying all findings and commit nothing.
- **REQ-UNC.MODEL.GRAPH.STORE-004** The store shall offer no command that deletes or overwrites a stored model version, and `resolve(ModelRef)` shall fail with `digest_mismatch` if the stored digest differs from the reference.
- **REQ-UNC.MODEL.GRAPH.STORE-005** When `unc.validate_model@1` runs, the system shall return the findings and the digest without storing anything.

## Acceptance criteria

- **AC-1** Given `fixtures/unc/model/graph/bore_ls.json`, when `fid unc create-model --file <handle> --json` runs, then the model is stored with the expected digest and one `unc.create_model@1` event is journaled; running it again commits nothing new.
- **AC-2** (error path) Given `bore_ls_param.json` with the same name and semver, then `version_conflict` and the journal head is unchanged.
- **AC-3** (error path) Given `bad/cycle.json`, then `model_invalid` with the `cycle` finding.
- **AC-4** Given `fid unc show-model --ref um:bore-ls@1.0.0 --json`, then the stored model and digest are returned; given a reference with a wrong digest, then `digest_mismatch`.
- **AC-5** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `unc.create_model@1` (W2; `file: ImportHandle | inline`); `unc.validate_model@1` (R1; same input; returns `{digest, findings}`); `unc.show_model@1` (R0; `ref: ModelRef | name@semver`).
- **Data structures:** `StoredModel { model_ref, author_kind: human|agent|script, created_entry }`, blob in content-addressed store.
- **Errors:** `version_conflict`, `model_invalid{findings}`, `model_not_found`, `digest_mismatch`.
- **Determinism:** D0.
- **Performance budget:** create of a 1 000-node model under 100 ms [unverified until measured].
- **Crate:** `fid-unc` (module `model_store`).

## Test fixtures

`fixtures/unc/model/graph/` (shared with siblings).

## Build prompt

> In crate `fid-unc`, module `model_store`, implement registry commands `unc.create_model@1` (W2; CLI `fid unc create-model`; MCP `unc_create_model`), `unc.validate_model@1` (R1) and `unc.show_model@1` (R0), with `--json` and `--dry-run`. Parse and digest with `fid-kunc` `model`, validate with `model::validate` against the current kernel manifest (`model_invalid{findings}`), store blobs content-addressed keyed by (name, semver) and digest, record author principal kind; same (name, semver) with a different digest → `version_conflict` (idempotent on the same digest); no delete or overwrite command; `resolve(ModelRef)` fails `digest_mismatch` on disagreement. Tests AC-1..AC-5. No panic or `unwrap`; BTreeMap only.

## Open questions

- None.

## Children

_Leaf: none._
