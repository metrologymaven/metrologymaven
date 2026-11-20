---
id: CORE.PRJ.STAGE.SWITCH
title: Active stage and per-stage value cache
level: 4
parent: CORE.PRJ.STAGE
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-project, fid-project-cmd]
built_pr: 354
satisfies: [REQ-CORE.PRJ.STAGE-002, REQ-CORE.PRJ.STAGE-005, REQ-CORE.PRJ.STAGE-006]
covers: []
depends_on: [CORE.PRJ.STAGE.MODEL, CORE.PRJ.TREE.MODEL, CORE.BUS.PIPE.HANDLER, CORE.REG.DEF, CORE.STO.CACHE]
side_effect: W2
determinism: D0
cli: [fid project set-active-stage]
mcp: [project_set_active_stage]
journal: [project.set_active_stage@1]
updated: 2026-11-17
---

# CORE.PRJ.STAGE.SWITCH: Active stage and per-stage value cache

## Purpose

The active stage is document state (results and reports depend on it), so switching it is a W2 command. Switching presents every element's value for the new stage from its per-stage slot, without recomputing elements whose inputs for that stage are unchanged; values are loaded from the content-addressed slot digests through an evictable cache.

## Scope

**In:** `project.set_active_stage@1`; the `StageContext` read API (active stage, slot lookup); the per-stage value cache keyed by slot digest; the switch-time budget.
**Out:** stage entities and slots (`CORE.PRJ.STAGE.MODEL`); recompute of outdated slots (`CORE.DEP.RECOMP`); viewer refresh (`VIZ`); cache storage and eviction (`CORE.STO.CACHE`).

## Requirements

- **REQ-CORE.PRJ.STAGE.SWITCH-001** When `project.set_active_stage@1` runs with a non-removed stage, the project model shall set it as the active stage in one W2 entry.
- **REQ-CORE.PRJ.STAGE.SWITCH-002** When the active stage changes, the project model shall present each element's value from that stage's slot and shall start no recompute for elements whose slot is `current`.
- **REQ-CORE.PRJ.STAGE.SWITCH-003** If the target stage is removed or unknown, then the command shall return `validation` with reason `bad_stage` and commit nothing.
- **REQ-CORE.PRJ.STAGE.SWITCH-004** Switching the active stage of a 100,000-element project shall complete in under 200 ms when no recompute is needed, on reference hardware [unverified until measured].
- **REQ-CORE.PRJ.STAGE.SWITCH-005** A cache miss shall load the value from the content-addressed store by slot digest and shall never recompute it.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/prj/stage/switch/p3s` (3 stages, 50 elements, all slots `current` with distinct values), when `fid project set-active-stage --stage <s2> --json` runs, then `fid project describe-element <e1> --json` shows s2's value and no recompute job is created.
- **AC-2** Given benchmark `bench_switch_100k` (100,000 elements, 2 stages, warm cache), then a switch takes under 200 ms [unverified until measured].
- **AC-3** (error path) Given a removed stage, then `validation` / `bad_stage` and nothing is journaled.
- **AC-4** Given the cache cleared, when switching, then values are loaded from the store and equal those before; no handler runs.
- **AC-5** (replay) Given the journal of AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `project.set_active_stage@1` W2, D0, undo `head_move`; params `{ stage: StageId }`; result `{ stage }`.
- **Data structures:** `StageContext { active: StageId }`; `fn value(element, stage) -> Result<ValueRef, _>` via cache keyed by slot digest.
- **Algorithm:** the handler validates and emits `ActiveStageSet`; applying it swaps the active-stage pointer only (O(1)); value reads resolve lazily through the cache.
- **Errors:** `validation` (`bad_stage`), `blob_missing` (from the store).
- **Determinism:** D0.
- **Performance budget:** REQ-004.
- **Crate:** `fid-project` (module `stage_switch`).

## Test fixtures

`fixtures/core/prj/stage/switch/p3s` and the benchmark generator, committed with fixed seeds; CC-BY-4.0.

## Build prompt

> In crate `fid-project`, add module `stage_switch` and register `project.set_active_stage@1` (W2, D0, undo head_move; CLI `fid project set-active-stage`, MCP `project_set_active_stage`). The handler checks the stage exists and is not removed (`bad_stage`) and emits `ActiveStageSet`; applying it swaps an active-stage pointer only. Provide `StageContext` with lazy value lookup by slot digest through the `fid-store` cache, loading from the content-addressed store on a miss and never recomputing. Add `bench_switch_100k`. Tests: AC-1..AC-5 with `fixtures/core/prj/stage/switch/p3s`. Constraints: CONSTITUTION C1 (no value recomputed outside the kernel path), §8.

## Open questions

- Active stage as document state (W2) rather than view state still needs confirmation with `VIZ`.
- Build: the CLI `fid project set-active-stage` / `describe-element` and MCP surfaces are not wired (generated from the `CommandDef` once the surface emitters exist, as for #300 and #347), and the bus commit stage (`CORE.BUS.PIPE.STAGE`) and replay loop (`CORE.RPL.EXEC.LOOP`) are unbuilt, so AC-1 and AC-5 are tested at the handler level (events folded into the tree state; replay re-runs the handler) and AC-1's "describe-element" is `StageContext::value`.
- Build: the split follows #347: the command logic, `StageContext` and the cache traits are in `fid-project` (module `stage_switch`, layer model, no store); the bus handler and definition are in `fid-project-cmd` (`stage::StageOp::SetActive`), and the adapters over the `fid-store` blob store and cache in `fid-project-cmd` (module `stage_switch`).
- Build: the slot digest is the value's content address (SHA-256 of the value bytes, the `CORE.STO.CAS` blob digest). Values are single blobs (up to 8 MiB); resolving a chunked object's slot digest is not done here.
- Build: the cache is never a source of truth. Every value is read by the slot digest the tree holds at the time of the read; cache bytes are hashed and compared with it, and an entry that does not match is removed, counted (`ReadStats::stale`) and never served; store bytes that do not match are `blob_corrupt`. There is no compute path: a slot without a digest is `no_value`, and a miss loads from the store (REQ-005).
- Build: `fid-store`'s cache has only memo and snapshot keys, so values are cached under the memo key of the pure read `project.stage_value@1` whose input is the slot digest (`fid_project_cmd::stage_switch::value_key`); a dedicated key form would belong to `CORE.STO.CACHE`. `fid-project` also offers a bounded in-memory LRU `MemoryValueCache`.
- Build: `ActiveStageSet` swaps the active-stage pointer only (and patches the stage list entity in place): no slot, value or stage record moves or merges, so every per-stage digest is unchanged. The target must be a live stage (`bad_stage` otherwise, including the reserved default ID after adoption); the active stage itself is accepted (a no-op switch is still one W2 entry).
- Build: AC-2 is measured at the model level (`bench_switch_100k`: `Tree::apply` of the switch on 100,000 elements, under 200 ms in a debug build). Through the bus `TreeState`, a commit also copies the state for rollback, which is O(elements) [unverified on reference hardware].
- Build: value read refusals map to bus catalogue v3 (`fid_project_cmd::stage_switch::to_bus_error`): `bad_reference` and `stage_not_found` are `validation` (`bad_reference`, `bad_stage`), `no_value` is `not_found/unknown_entity`, `blob_missing`, `blob_corrupt` and `storage` are `internal/storage`.

## Children

_Leaf: none._
