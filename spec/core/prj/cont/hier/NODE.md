---
id: CORE.PRJ.CONT.HIER
title: Container entities and hierarchy commands
level: 4
parent: CORE.PRJ.CONT
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-project, fid-project-cmd]
built_pr: 300
satisfies: [REQ-CORE.PRJ.CONT-001, REQ-CORE.PRJ.CONT-002, REQ-CORE.PRJ.CONT-008]
covers: []
depends_on: [CORE.PRJ.CONT.ID, CORE.BUS.PIPE.HANDLER, CORE.REG.DEF]
side_effect: [W2, W2, W2, W2]
determinism: D0
cli: [fid project create-part, fid project create-revision, fid project create-plan, fid project create-run]
mcp: [project_create_part, project_create_revision, project_create_plan, project_create_run]
journal: [project.create_part@1, project.create_revision@1, project.create_plan@1, project.create_run@1]
updated: 2026-11-17
---

# CORE.PRJ.CONT.HIER: Container entities and hierarchy commands

## Purpose

The container entities of CONSTITUTION §5, Part → Revision → Plan → Run, and the W2 commands that create them. A Revision belongs to one Part, a Plan to one Revision, and a Run references one Plan and that Plan's Revision. The handlers enforce the hierarchy so no Run can mix a Plan of one Revision with another Revision.

## Scope

**In:** `Part`, `Revision`, `Plan`, `Run` types (ID, label as untrusted text, parent reference, created-by principal); `project.create_part@1`, `project.create_revision@1`, `project.create_plan@1`, `project.create_run@1`; hierarchy invariants; a `StateView` read helper for the hierarchy.
**Out:** project create/open/close/save-as (`CORE.PRJ.CONT.LIFE`); IDs (`CORE.PRJ.CONT.ID`); plan contents (`GDT`, `CMM`, `AUTO`); stages under a Run (`CORE.PRJ.STAGE`).

## Requirements

- **REQ-CORE.PRJ.CONT.HIER-001** The project model shall represent exactly the entity kinds Part, Revision, Plan and Run, where a Revision has one parent Part, a Plan one parent Revision, and a Run one Plan and one Revision.
- **REQ-CORE.PRJ.CONT.HIER-002** If `project.create_run@1` names a Plan whose Revision differs from the named Revision, then the command shall return `validation` with reason `hierarchy` and commit nothing.
- **REQ-CORE.PRJ.CONT.HIER-003** If a create command names a parent that does not exist or is of the wrong kind, then it shall return `validation` with reason `bad_reference` and commit nothing.
- **REQ-CORE.PRJ.CONT.HIER-004** When a create command succeeds, the new entity shall get a UUIDv7 from the ID service, or the imported ID when the call supplies one.
- **REQ-CORE.PRJ.CONT.HIER-005** The four create commands shall be registry commands of side-effect class W2, undoable by head move, executed through the bus.
- **REQ-CORE.PRJ.CONT.HIER-006** Labels shall be untrusted text of 1 to 256 characters and need not be unique.

## Acceptance criteria

- **AC-1** Given an empty project from fixture `fixtures/core/prj/cont/hier/empty`, when `fid project create-part --label "Bracket"`, then `create-revision --part <p> --label B`, `create-plan --revision <r>`, `create-run --plan <pl> --revision <r>` run with `--json`, then four W2 entries exist and each entity references its parent.
- **AC-2** (error path) Given two revisions r1 and r2 and a plan of r1, when `create-run --plan <pl> --revision <r2>` runs, then `validation` / `hierarchy` and nothing is journaled.
- **AC-3** (error path) Given a create-plan naming a Part ID as its revision, then `validation` / `bad_reference`.
- **AC-4** (replay) Given the journal of AC-1, when replayed in exact mode, then the outcome is `IDENTICAL` and all IDs are equal; after `fid journal undo`, the Run is gone.

## Design

- **Registry command(s):**
  - `project.create_part@1` W2, D0; params `{ label, id?: ImportedId }`; result `{ part }`.
  - `project.create_revision@1` W2, D0; params `{ part: EntityRef<Part>, label, id? }`.
  - `project.create_plan@1` W2, D0; params `{ revision: EntityRef<Revision>, label, id? }`.
  - `project.create_run@1` W2, D0; params `{ plan: EntityRef<Plan>, revision: EntityRef<Revision>, label, id? }`.
- **Data structures:** container structs with `Untrusted<String>` labels; event `ContainerCreated { kind, id, parent, label }`.
- **Algorithm:** handler checks references through the state view, checks the plan-revision rule, asks the ID service for an ID, returns the event.
- **Errors:** `validation` (`hierarchy`, `bad_reference`, `label`), `id_collision`.
- **Determinism:** D0 (IDs are recorded observations).
- **Performance budget:** < 1 ms per command excluding commit [unverified until measured].
- **Crate:** `fid-project` (module `container`).

## Test fixtures

`fixtures/core/prj/cont/hier/empty`, an empty project from a committed generator; CC-BY-4.0.

## Build prompt

> In crate `fid-project`, add module `container` with `Part`, `Revision`, `Plan`, `Run` (ID, untrusted label 1..256 chars, parent reference, created-by) and register `project.create_part@1`, `project.create_revision@1`, `project.create_plan@1`, `project.create_run@1` (all W2, D0, undo head_move; CLI `fid project create-part | create-revision | create-plan | create-run`; MCP `project_create_*`). Handlers read the state view, check parent existence and kind (`bad_reference`) and that a Run's Plan belongs to the named Revision (`hierarchy`), get an ID from `id::IdService` (UUIDv7 or the imported ID), and return a `ContainerCreated` event. Tests: AC-1..AC-4 with `fixtures/core/prj/cont/hier/empty`. Constraints: CONSTITUTION §5 (Part → Revision → Plan → Run, stable IDs), C3, C6, §8.

## Open questions

- The parent `CORE.PRJ.CONT` listed no `project.create_part@1`; it is added here because REQ-CORE.PRJ.CONT-001 needs Parts to exist.
- Build (PR #300): the CLI `fid project create-*` and MCP `project_create_*` surfaces are not wired yet; they are to be generated from the four `CommandDef`s once the registry surface generator exists. The bus commit stage (`CORE.BUS.PIPE.STAGE`), journal undo (`CORE.JRN.UNDO.MOVE`) and the replay loop (`CORE.RPL.EXEC.LOOP`) are not built either, so AC-1 and AC-4 are tested at the handler level: the four events are folded into an in-memory state view, undo is the state at the previous head, and replay re-runs the handlers with a `ReplaySource` of the recorded observations. The end-to-end form of AC-1 and AC-4 remains for those leaves.
- Build (PR #300): the leaf is split over two crates so `fid-project` stays in layer model without the bus (and its SQLite): `fid-project` (module `container`) holds the types, labels, the checks and the `Hierarchy` helper; the engine crate `fid-project-cmd` holds the bus handlers, the registry definitions and `register` (the `fid-geo-cmd` pattern of #302).
- Build (PR #300): an imported (or drawn) ID already in the project is `conflict/id_collision`, a reason added to the bus error catalogue as frozen baseline v2 (additions only; v1 unchanged). GEO.TYPES.REG (#302) uses the same reason.
- Build (PR #300): AC-1 creates the plan and the run without a label while Design lists `label` for every command; `label` is required for part and revision and optional for plan and run (stored as `null`).
- Build (PR #300): label rule, checked by the handler only: 1 to 256 Unicode scalar values, no Cc control character, no bidi control (U+202A..U+202E, U+2066..U+2069, U+200E, U+200F, U+061C) and NFC, the text rules of TRUST.PROFILE.DEFS (#290), because labels are journaled for good; any failure is `validation/label`. The published `params_schema` declares the label as an untrusted string with no `minLength` or `maxLength`, so a too short or too long label gets the specific `label` reason instead of `schema`.

## Children

_Leaf: none._
