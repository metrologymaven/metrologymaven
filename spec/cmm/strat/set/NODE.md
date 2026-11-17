---
id: CMM.STRAT.SET
title: Feature sets with shared strategy settings
level: 3
parent: CMM.STRAT
kind: feature
status: leaf
phase: P2
size: S
owners: []
claimed_by:
satisfies: [REQ-CMM.STRAT-008]
covers: [PCD.FEA.04]
depends_on: [CMM.STRAT.FEAT]
side_effect: W2
determinism: D0
cli: [fid cmm create-feature-set, fid cmm set-feature-set-strategy]
mcp: [cmm_create_feature_set, cmm_set_feature_set_strategy]
journal: [cmm.create_feature_set@1, cmm.set_feature_set_strategy@1]
updated: 2026-11-16
---

# CMM.STRAT.SET: Feature sets with shared strategy settings

## Purpose

Groups features into a named set whose members share strategy settings, and applies a change to the shared settings to every member in one journaled command. This lets a programmer change, for example, the hit count of twenty holes at once, with one undoable journal entry.

## Scope

**In:**
- `FeatureSet { id, name, members, shared: StrategyPatch }`
- `cmm.create_feature_set@1` and `cmm.set_feature_set_strategy@1` (both W2)
- Applicability check of a shared setting to every member's strategy type
- Atomic application: all members change or none

**Out** (and which node owns it instead):
- Strategy types and validation (owned by `CMM.STRAT.FEAT` and `CMM.STRAT.CHECK`)
- Measurement order of members (owned by `CMM.PATH`)

## Requirements

- **REQ-CMM.STRAT.SET-001** The system shall create a feature set from one to 10,000 existing features with a stable ID and an untrusted-text name, and shall reject a feature that is already in another set with `SET_MEMBER_CONFLICT`.
- **REQ-CMM.STRAT.SET-002** When shared settings are changed, the system shall apply the change to every member's strategy in one journaled command, re-validating each member through its strategy type.
- **REQ-CMM.STRAT.SET-003** If a shared setting does not apply to a member's strategy type, or any member's re-validation fails, then the system shall reject the whole change with `SET_FIELD_NOT_APPLICABLE` or the member's validation error, naming the members, and change no member.
- **REQ-CMM.STRAT.SET-004** The command result shall list each member's strategy digest before and after the change.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/cmm/strat/plate_12_holes.json` (twelve circle features with strategies), when `fid cmm create-feature-set --features <12 ids> --name holes --json` then `fid cmm set-feature-set-strategy --set <id> --hits 8 --json` run, then all twelve strategies have eight hits, one journal entry records the change, and the result lists twelve before and after digests.
- **AC-2** Given AC-1, when `fid journal undo` runs, then all twelve strategies return to their previous digests.
- **AC-3** (error path) Given a set mixing circles and planes, when `--levels 3` (not applicable to planes) is applied, then the command fails with `SET_FIELD_NOT_APPLICABLE` naming the plane members and no strategy changes.
- **AC-4** (error path) Given AC-1, when a set is created that includes one of the twelve holes, then it fails with `SET_MEMBER_CONFLICT`.
- **AC-5** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry commands:** `cmm.create_feature_set@1` (W2; params `name: UntrustedText`, `features: Vec<FeatureId>`; result `{set_id}`); `cmm.set_feature_set_strategy@1` (W2; params `set_id`, `patch: StrategyPatch`; result `{members: Vec<{feature_id, before, after}>}`).
- **Data structures:** `FeatureSet`; `StrategyPatch` is an ordered field-to-value map over strategy parameter names.
- **Algorithm:** validate every member against the patch first (members in ascending feature-ID order), then commit all in one transaction.
- **Errors:** `SET_MEMBER_CONFLICT`, `SET_FIELD_NOT_APPLICABLE { members }`, `SET_TOO_LARGE`, `FEATURE_NOT_FOUND`, plus member validation errors.
- **Determinism:** D0.
- **Crate:** `fid-cmm-strat`.

## Test fixtures

`fixtures/cmm/strat/plate_12_holes.json` (synthetic plate with twelve circle features and a plane), hand-written, CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-cmm-strat`, implement feature sets: commands `cmm.create_feature_set@1` and `cmm.set_feature_set_strategy@1` (both W2, journaled; CLI `fid cmm create-feature-set`, `fid cmm set-feature-set-strategy` with `--json`; MCP generated).
> A set holds 1-10,000 features, each in at most one set (`SET_MEMBER_CONFLICT`). A shared-settings change is validated for every member first (`SET_FIELD_NOT_APPLICABLE {members}` or the member's own validation error) and then applied to all members in one journal entry, or to none. The result lists before and after strategy digests per member; undo restores all.
> Acceptance tests: the five criteria in spec node `CMM.STRAT.SET`, fixture `fixtures/cmm/strat/plate_12_holes.json`.
> Constraints: no panic, no `unsafe`, no `unwrap`/`expect`; no std HashMap or HashSet; determinism D0.

## Open questions

- None.

## Children

_Leaf: none._
