---
id: CORE.PRJ.TPL.CREATE
title: Create a project from a template
level: 4
parent: CORE.PRJ.TPL
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.PRJ.TPL-001, REQ-CORE.PRJ.TPL-004, REQ-CORE.PRJ.TPL-005, REQ-CORE.PRJ.TPL-006]
covers: [GOM.PRJ.01, PW.PRJ.05]
depends_on: [CORE.PRJ.TPL.PKG, CORE.PRJ.CONT.LIFE, CORE.PRJ.CONT.ID, CORE.PRJ.TREE.MODEL, CORE.PRJ.META, CORE.BUS.PIPE.HANDLER, CORE.REG.DEF]
side_effect: W2
determinism: D0
cli: [fid project create-from-template]
mcp: [project_create_from_template]
journal: [project.create_from_template@1]
updated: 2026-11-17
---

# CORE.PRJ.TPL.CREATE: Create a project from a template

## Purpose

Starts a new project from a template so a team inspects every part of a family the same way. The new project copies the template's plan, element definitions, groups, tags and metadata; its element IDs are UUIDv5 values derived from the template ID and the template's element IDs, so the same template always yields the same IDs; and it records the template ID so reports can show which template was used.

## Scope

**In:** `project.create_from_template@1`; integrity check before use; ID derivation; recording the template reference in the new project; states of copied elements (`not_computed`).
**Out:** the package format and loader (`CORE.PRJ.TPL.PKG`); creating the empty project (`CORE.PRJ.CONT.LIFE` API); showing the template in reports (`RPT`).

## Requirements

- **REQ-CORE.PRJ.TPL.CREATE-001** When `project.create_from_template@1` runs, the system shall create a new project (as `project.create_project@1` does) and copy the template's plan, element definitions, groups, tags and metadata into it in the same commit.
- **REQ-CORE.PRJ.TPL.CREATE-002** Each copied entity's ID shall be UUIDv5(template namespace, template entity ID), where the template namespace is UUIDv5 of the fixed constant `TEMPLATE_ROOT_NS` and the template ID, so it does not depend on the new project.
- **REQ-CORE.PRJ.TPL.CREATE-003** The new project shall record the template ID and label as project metadata keys `template.id` and `template.label`.
- **REQ-CORE.PRJ.TPL.CREATE-004** If the template fails integrity, then the command shall return `integrity` and create nothing.
- **REQ-CORE.PRJ.TPL.CREATE-005** Every copied element shall start in state `not_computed`.
- **REQ-CORE.PRJ.TPL.CREATE-006** The command shall be side-effect class W2 executed through the bus, with the target project folder given as a handle in an approved root.

## Acceptance criteria

- **AC-1** Given fixture template `fixtures/core/prj/tpl/create/bracket` (40 element definitions), when `fid project create-from-template --template root:templates/bracket --target root:work/b1 --json` runs, then the new project has 40 elements in `not_computed`, the template's groups, tags and metadata, and `template.id` equals the template ID.
- **AC-2** Given the same template instantiated twice into b1 and b2, then the element IDs are identical in both and equal fixture `expected_ids.json`.
- **AC-3** (error path) Given a tampered template, then `integrity` and no folder is created.
- **AC-4** (replay) Given the journal of AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `project.create_from_template@1` W2, D0, `fs_effect: write_root`; params `{ template: FileHandle, target: FileHandle, label? }`; result `{ project, template_id }`.
- **Data structures:** `TEMPLATE_ROOT_NS` (a fixed UUID constant published in the crate); template namespace = UUIDv5(`TEMPLATE_ROOT_NS`, template ID); entity ID = UUIDv5(template namespace, template entity ID).
- **Algorithm:** load and verify the template; create the project through the lifecycle API; emit creation events for groups and elements with derived IDs and `not_computed` slots; emit metadata events.
- **Errors:** `integrity`, `policy` (`would_overwrite`, `raw_path_refused`), `validation` (`unregistered_type` when a template uses a type no installed domain registers).
- **Determinism:** D0.
- **Performance budget:** instantiate a 10,000-element template in < 2 s [unverified until measured].
- **Crate:** `fid-project` (module `template_create`).

## Test fixtures

`fixtures/core/prj/tpl/create/bracket` and `expected_ids.json`, from a committed generator; CC-BY-4.0.

## Build prompt

> In crate `fid-project`, add module `template_create` and register `project.create_from_template@1` (W2, D0, `fs_effect: write_root`; CLI `fid project create-from-template`, MCP `project_create_from_template`). Load the template with `template_pkg` (`integrity` on failure, nothing created); create the project through the lifecycle API; copy plan, element definitions, groups, tags and metadata with IDs = UUIDv5(UUIDv5(TEMPLATE_ROOT_NS, template ID), template entity ID); every element starts `not_computed`; record `template.id` and `template.label` metadata. Types not registered by an installed domain fail with `unregistered_type`. Tests: AC-1..AC-4 with `fixtures/core/prj/tpl/create/`. Constraints: CONSTITUTION §5 (derived IDs are UUIDv5), §3 (handles), §8.

## Open questions

- REQ-CORE.PRJ.TPL-004 says IDs derive from "the template digest and the template's element IDs"; using a fixed root namespace rather than each project's namespace keeps IDs equal across projects made from one template, as that requirement intends.

## Children

_Leaf: none._
