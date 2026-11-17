---
id: UNC.MODEL.BIND
title: Model binding to characteristics and human adoption
level: 3
parent: UNC.MODEL
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.MODEL-007, REQ-UNC.MODEL-004]
covers: []
depends_on: [UNC.MODEL.GRAPH, CORE]
side_effect: W2
determinism: D0
cli: [fid unc bind-model, fid unc adopt-model]
mcp: [unc_bind_model]
journal: [unc.bind_model@1, unc.adopt_model@1]
updated: 2026-11-16
---

# UNC.MODEL.BIND: Model binding to characteristics and human adoption

## Purpose

Connects a validated Uncertainty Model version to the characteristics of a plan, so that propagation knows which model to evaluate for which result, and records who authored and who adopted each model version. Agent-drafted or agent-edited models are proposals; results under them cannot be released until a human adopts the exact model digest.

## Scope

**In:**
- `unc.bind_model@1` (W2): bind `um:<name>@<semver>#<digest>` to one or more characteristic IDs in a plan, with input mapping (model input ID to plan quantity or contributor parameter)
- `unc.adopt_model@1` (W2, human principals only): adopt a specific model digest
- Provenance of model versions: author principal kind, adoption record
- Release check function `model_releasable(digest) -> bool` used by the release gate

**Out:**
- Model creation and validation (owned by `UNC.MODEL.GRAPH`)
- Agent branch routing and promotion (owned by `CORE.BUS` and `CORE.JRN`)
- The release gate itself (owned by `TRUST`)

## Requirements

- **REQ-UNC.MODEL.BIND-001** When `unc.bind_model@1` is run with a model reference that has passed validation and a list of characteristic IDs, the system shall record the binding with the model's semver and digest and commit one journal event.
- **REQ-UNC.MODEL.BIND-002** If the model reference does not resolve to a validated model, any characteristic ID does not exist, or a model input is left unmapped, then the command shall fail with `model_not_valid`, `char_not_found` or `input_unmapped` and commit nothing.
- **REQ-UNC.MODEL.BIND-003** When a model version was created or last edited by an agent principal, the system shall mark it `agent_proposal` and `model_releasable` shall return false until a human adopts that exact digest.
- **REQ-UNC.MODEL.BIND-004** If `unc.adopt_model@1` is called by an agent principal, then the command shall fail with `principal_not_human` and commit nothing; `unc.adopt_model@1` shall not be exposed as an MCP tool.
- **REQ-UNC.MODEL.BIND-005** When a binding is changed to a new model version, existing results shall keep their original model reference, and the new binding shall apply only to results computed after the change.

## Acceptance criteria

- **AC-1** Given `fixtures/unc/bind/bore.fidproj` (one diameter characteristic `C-010`) and validated model `um:bore-ls@1.0.0` from `fixtures/unc/bind/um_bore_ls.json`, when `fid unc bind-model --model um:bore-ls@1.0.0 --char C-010 --map d:C-010.value --json` runs as a human, then the binding is stored with the model digest and one `unc.bind_model@1` event is journaled.
- **AC-2** (error path) Given the same project, when binding with an unmapped input, then it fails with `input_unmapped` and the journal head is unchanged.
- **AC-3** (agent proposal) Given the model created through MCP by an agent principal, when `model_releasable` is queried, then it returns false; after a human runs `fid unc adopt-model --model um:bore-ls@1.0.0#<digest>`, it returns true.
- **AC-4** (error path) Given an agent principal calling adopt through the control channel, then it fails with `principal_not_human`; and the MCP tool list generated from the registry contains no `unc_adopt_model`.
- **AC-5** (versioning) Given a result computed under `@1.0.0`, when the binding moves to `@1.1.0`, then the old result still references `@1.0.0` and its digest.
- **AC-6** (replay) Given the journal from AC-1 and AC-3, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):**
  - `unc.bind_model@1`, W2. Params: `model: ModelRef`, `chars: Vec<EntityId>`, `map: Vec<(input_id, QuantityPath)>`. Result: `{ binding_id, model_digest }`.
  - `unc.adopt_model@1`, W2 with registry flag `human_only` (excluded from MCP projection; agents get `principal_not_human`). Params: `model: ModelRef` with digest. Result: `{ adopted_by, adopted_at }`.
- **Data structures:** `ModelBinding { binding_id (UUIDv7), plan_id, chars, model: ModelRef, map }`; `ModelProvenance { author_kind: Human | Agent, adopted_by: Option<Principal>, adopted_at: Option<JournalTime> }`.
- **Algorithm:** validate references and mapping against the plan and model input list (sorted by input ID), store binding, commit. Adoption checks principal kind from the bus context, then records adoption keyed by digest.
- **Errors:** `model_not_valid`, `char_not_found`, `input_unmapped`, `principal_not_human`, `digest_mismatch`.
- **Determinism:** D0.
- **Performance budget:** bind 1,000 characteristics in under 100 ms excluding fsync [unverified until measured].
- **Crate:** `fid-unc`.

## Test fixtures

`fixtures/unc/bind/bore.fidproj` (synthetic project from a test helper) and `fixtures/unc/bind/um_bore_ls.json` (a two-input model: diameter from a least-squares circle fit plus a thermal input), both CC-BY-4.0.

## Build prompt

> In crate `fid-unc`, implement registry commands `unc.bind_model@1` (W2; CLI `fid unc bind-model`; MCP `unc_bind_model`; journal `unc.bind_model@1`) and `unc.adopt_model@1` (W2 with the registry `human_only` flag so the MCP projection omits it; CLI `fid unc adopt-model`; journal `unc.adopt_model@1`).
> Bind: resolve the model reference to a validated model (else `model_not_valid`), check every characteristic exists (`char_not_found`) and every model input is mapped (`input_unmapped`), store `ModelBinding` with the model semver and digest, commit one event. Never rewrite the model reference on existing results when a binding changes.
> Provenance: a model version created or edited by an agent principal is `agent_proposal`; expose `model_releasable(digest)` returning false until a human adopts that exact digest. Adopt: reject agent principals with `principal_not_human` (CONSTITUTION C5: agents never adopt their own proposals); record adopter and journal time.
> Tests with `fixtures/unc/bind/bore.fidproj` and `um_bore_ls.json`: happy path; unmapped input; agent proposal blocked then adopted by a human; agent adopt refused and absent from the MCP tool list; old results keep `@1.0.0`; exact replay IDENTICAL. Support `--json` and `--dry-run`. No panics, no `unwrap`.

## Open questions

- Whether adoption should instead be a generic CORE.JRN promotion of the agent branch that contains the model is for CORE to confirm; this leaf keeps an explicit digest-level adoption so the release gate has one fact to check.

## Children

_Leaf: none._
