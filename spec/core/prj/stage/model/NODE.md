---
id: CORE.PRJ.STAGE.MODEL
title: Stage entity, per-stage slots and stage commands
level: 4
parent: CORE.PRJ.STAGE
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-project, fid-project-cmd]
built_pr: 347
satisfies: [REQ-CORE.PRJ.STAGE-001, REQ-CORE.PRJ.STAGE-003, REQ-CORE.PRJ.STAGE-004, REQ-CORE.PRJ.STAGE-006]
covers: [GOM.PRJ.02, PW.PRJ.02]
depends_on: [CORE.PRJ.CONT.HIER, CORE.PRJ.CONT.ID, CORE.PRJ.TREE.MODEL, CORE.BUS.PIPE.HANDLER, CORE.REG.DEF]
side_effect: [W2, W2, W2, R0]
determinism: D0
cli: [fid project add-stage, fid project remove-stage, fid project reorder-stages, fid project list-stages]
mcp: [project_add_stage, project_remove_stage, project_reorder_stages, project_list_stages]
journal: [project.add_stage@1, project.remove_stage@1, project.reorder_stages@1, project.list_stages@1]
updated: 2026-11-17
---

# CORE.PRJ.STAGE.MODEL: Stage entity, per-stage slots and stage commands

## Purpose

Lets one project hold many measured instances (parts, serials, repeated measurements) against one shared plan: the stage entity under a Run, a per-stage data slot on every element while element definitions stay shared, and the commands to add, remove (soft, undoable), reorder and list stages. This is the CORE answer to GOM stages and PolyWorks multi-piece projects [unverified that the two concepts match].

## Scope

**In:** `Stage { id, run, label, order, serial, metadata }`; per-stage slots (`value digest`, `state`) on every element; `project.add_stage@1`, `project.remove_stage@1`, `project.reorder_stages@1`, `project.list_stages@1`; the 1,000-stage scale target.
**Out:** the active stage and value cache (`CORE.PRJ.STAGE.SWITCH`); recompute of a stage's elements (`CORE.DEP`); batch import into many stages (`IO`, `AUTO`); trends and SPC (`RPT`).

## Requirements

- **REQ-CORE.PRJ.STAGE.MODEL-001** The project model shall support at least 1,000 stages in one project, each holding its own measured data against the shared plan [unverified that 1,000 suffices for incumbent parity].
- **REQ-CORE.PRJ.STAGE.MODEL-002** When `project.add_stage@1` runs, every existing element shall gain an empty slot for the new stage in state `not_computed`, in the same commit.
- **REQ-CORE.PRJ.STAGE.MODEL-003** When an element is created in a project with stages, it shall get one slot per non-removed stage, each in state `not_computed`.
- **REQ-CORE.PRJ.STAGE.MODEL-004** When `project.remove_stage@1` runs, the project model shall mark the stage removed, keep its data in the journal, hide it from `project.list_stages@1` unless `include_removed` is set, and allow undo by head move.
- **REQ-CORE.PRJ.STAGE.MODEL-005** If the removed stage is the active stage, then `project.remove_stage@1` shall return `precondition` with reason `stage_active` and commit nothing.
- **REQ-CORE.PRJ.STAGE.MODEL-006** When `project.reorder_stages@1` runs with a permutation of the non-removed stage IDs, the project model shall set their order to that permutation; if the list is not a permutation, then it shall return `validation`.
- **REQ-CORE.PRJ.STAGE.MODEL-007** Adding, removing and reordering stages shall be W2 registry commands executed through the bus, and listing shall be R0.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/prj/stage/model/p100` (100 elements, one stage), when `fid project add-stage --run <r> --label S2 --json` runs, then all 100 elements have a S2 slot in `not_computed` and one W2 entry exists.
- **AC-2** Given 1,000 stages added by a script to p100, then `fid project list-stages --json` returns 1,000 stages in order and each element has 1,000 slots.
- **AC-3** Given S2 removed, then list omits it, `--include-removed` shows it, and `fid journal undo` restores it with its data; (error path) removing the active stage returns `precondition` / `stage_active`.
- **AC-4** (error path) Given a reorder list with a missing or duplicate ID, then `validation` and nothing is journaled.
- **AC-5** (replay) Given the journal of AC-1 and AC-3, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):**
  - `project.add_stage@1` W2, D0; params `{ run: EntityRef<Run>, label, serial?: Untrusted<String> }`; result `{ stage }`.
  - `project.remove_stage@1` W2, D0; params `{ stage }`.
  - `project.reorder_stages@1` W2, D0; params `{ run, order: [StageId] }`.
  - `project.list_stages@1` R0; params `{ run?, include_removed?: bool }`.
- **Data structures:** `Stage`; element slots `BTreeMap<StageId, Slot { digest: Option<Digest>, state }>` held in the element record (`CORE.PRJ.TREE.MODEL` provides the slot field); events `StageAdded`, `StageRemoved`, `StagesReordered`.
- **Algorithm:** handler emits one event; slot creation is part of applying `StageAdded` to the state (no per-element events), so the entry stays small.
- **Errors:** `validation`, `precondition` (`stage_active`), `not_found`.
- **Determinism:** D0.
- **Performance budget:** add a stage to a 100,000-element project in < 300 ms [unverified until measured].
- **Crate:** `fid-project` (module `stage`).

## Test fixtures

`fixtures/core/prj/stage/model/p100`, a synthetic project from a committed generator; CC-BY-4.0.

## Build prompt

> In crate `fid-project`, add module `stage` with `Stage { id, run, label, order, serial, metadata, removed }` and register `project.add_stage@1`, `project.remove_stage@1`, `project.reorder_stages@1` (W2, D0, undo head_move) and `project.list_stages@1` (R0); CLI `fid project add-stage | remove-stage | reorder-stages | list-stages`, MCP `project_*`. Applying `StageAdded` gives every element a `not_computed` slot for the stage; new elements get one slot per non-removed stage. Removal is soft and undoable; removing the active stage fails with `stage_active`. Reorder requires an exact permutation (`validation`). Labels and serials are untrusted text. Support at least 1,000 stages. Tests: AC-1..AC-5 with `fixtures/core/prj/stage/model/p100`. Constraints: CONSTITUTION C3, C6, §5, §8, ordered collections only.

## Open questions

- PolyWorks multi-piece projects may differ from GOM stages [unverified]; confirm by seed verification before freezing.
- Build: the CLI `fid project add-stage | remove-stage | reorder-stages | list-stages` and MCP `project_*_stage(s)` surfaces are not wired; they come from the four `CommandDef`s once the registry surface generator exists (as for #300). The bus commit stage (`CORE.BUS.PIPE.STAGE`), journal undo (`CORE.JRN.UNDO.MOVE`) and the replay loop (`CORE.RPL.EXEC.LOOP`) are not built, so AC-1, AC-3 and AC-5 are tested at the handler level: events are folded into the tree state, undo is the state at the previous head, and replay re-runs the handlers with a `ReplaySource` of the recorded observations.
- Build: the split follows #300: model and command logic in `fid-project` (module `stage`, events applied by `tree`), bus handlers and definitions in `fid-project-cmd` (module `stage`).
- Build: the first `StageAdded` of a project that has only the implicit default stage of `CORE.PRJ.TREE.MODEL` takes its place: each element's default-stage slot moves, unchanged, to the new stage, which becomes the active stage. "A project with one stage" (p100) therefore has one stage whether or not it was named, and no data is lost. AC-2 is read as 1,000 stages in total: the script adds S2 .. S1000 to p100's S1.
- Build: `order` is not stored on the stage; it is the stage's position in the project's stage sequence (removed stages keep their positions). A reorder names one run's live stages, which fill the positions those stages held.
- Build: a removed stage keeps every element's slot for it (its data) but takes no new slots, refuses slot changes (`bad_stage`) and gives new elements no slot. Stage events from a domain are refused at registration (`duplicate_event`).
- Build: errors map to the bus catalogue v3 without new reasons: `stage_active` is `precondition/stage_active`; a reorder that is not a permutation is `validation/bad_stage`; a stage that is not live is `not_found/unknown_entity` (model code `stage_not_found`, which `IO.REG.ENV.IMPORT` can reuse through `Tree::target_stage`); bad label or serial text is `validation/label`; a run that is not a Run is `validation/bad_reference`.
- Build: for `IO.REG.ENV.IMPORT` the tree offers `target_stage(Option<stage>)` (the live stage, or the active stage), `stages_labelled(label)` (lookup by name; labels need not be unique) and `stage_digest(stage)` (the per-stage data digest that REQ-IO.REG.ENV.IMPORT-003 compares).
- Build: like the container commands, `project.add_stage@1` also takes an optional imported `id`; omitted, a UUIDv7 is drawn. `metadata` is always `{}` until a command sets it.

## Children

_Leaf: none._
