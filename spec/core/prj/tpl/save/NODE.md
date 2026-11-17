---
id: CORE.PRJ.TPL.SAVE
title: Save a project as a template
level: 4
parent: CORE.PRJ.TPL
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.PRJ.TPL-002, REQ-CORE.PRJ.TPL-006]
covers: [GOM.PRJ.01]
depends_on: [CORE.PRJ.TPL.PKG, CORE.PRJ.TREE.MODEL, CORE.PRJ.META, CORE.PRJ.CONT.HIER, CORE.BUS.POL.FS, CORE.BUS.PIPE.HANDLER, CORE.REG.DEF]
side_effect: W1
determinism: D0
cli: [fid project save-as-template]
mcp: [project_save_as_template]
journal: [project.save_as_template@1]
updated: 2026-11-17
---

# CORE.PRJ.TPL.SAVE: Save a project as a template

## Purpose

Writes the current project state, stripped of measured data, stage data and journal history, as a template package into an approved export root. Writing a file into a local export root is W1 with `fs_effect: write_root` (CONSTITUTION §3.1). The same project state always yields the same template ID.

## Scope

**In:** `project.save_as_template@1`; the strip rules (what is kept and dropped); writing the package atomically into the export root; determinism of the output.
**Out:** the package format and digest (`CORE.PRJ.TPL.PKG`); export roots (`CORE.BUS.POL.FS`, `CORE.STO.SET`); publishing templates elsewhere (`ECO`).

## Requirements

- **REQ-CORE.PRJ.TPL.SAVE-001** When `project.save_as_template@1` runs, the system shall write a package containing the plan, element definitions (type, name, group, parameters), groups, tags, metadata and project-scope settings of the current head.
- **REQ-CORE.PRJ.TPL.SAVE-002** The package shall contain no measured data, no per-stage slot values, no stage entities, no Runs and no journal entries.
- **REQ-CORE.PRJ.TPL.SAVE-003** When the same project state is saved twice, the two packages shall have the same template ID, with the creation time excluded from the digested manifest content.
- **REQ-CORE.PRJ.TPL.SAVE-004** The command shall be side-effect class W1 with `fs_effect: write_root`, and if the target exists, then it shall return `policy` with reason `would_overwrite` and write nothing.
- **REQ-CORE.PRJ.TPL.SAVE-005** If writing fails part way, then no partial package shall remain in the root.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/prj/tpl/save/measured` (a project with 2 stages of measured data, 40 elements, tags and metadata), when `fid project save-as-template --target root:templates/bracket --label Bracket --json` runs, then the package loads with `Template::load`, holds 40 element definitions with tags and metadata, and contains no measured blob, stage or Run.
- **AC-2** Given the same project saved twice to two targets, then both IDs are equal.
- **AC-3** (error path) Given an existing target, then `policy` / `would_overwrite`; given an agent raw path, then `policy` / `raw_path_refused`.
- **AC-4** (error path) Given a write failure injected after the second blob, then the target folder does not exist afterwards.

## Design

- **Registry command(s):** `project.save_as_template@1` W1, D0, `fs_effect: write_root`; params `{ target: FileHandle, label }`; result `{ template_id, handle }`.
- **Data structures:** `StripPlan` listing kept event and entity kinds; manifest from `CORE.PRJ.TPL.PKG` with `created` stored outside the digested part.
- **Algorithm:** project the head state; drop slots, stages, Runs and measured payload references; collect definition blobs; write into a temporary sibling folder; rename atomically into place.
- **Errors:** `policy` (`would_overwrite`, `raw_path_refused`, `outside_root`), `io`.
- **Determinism:** D0.
- **Performance budget:** save a 10,000-element template in < 2 s [unverified until measured].
- **Crate:** `fid-project` (module `template_save`).

## Test fixtures

`fixtures/core/prj/tpl/save/measured`, from a committed generator; CC-BY-4.0.

## Build prompt

> In crate `fid-project`, add module `template_save` and register `project.save_as_template@1` (W1, D0, `fs_effect: write_root`; CLI `fid project save-as-template`, MCP `project_save_as_template`). Keep plan, element definitions, groups, tags, metadata and project-scope settings of the head; drop measured data, per-stage slots, stages, Runs and journal entries. Build the manifest with `template_pkg` so equal state gives an equal ID (creation time outside the digested part). Write into a temporary sibling folder in the approved export root and rename atomically; never overwrite (`would_overwrite`). Tests: AC-1..AC-4 with `fixtures/core/prj/tpl/save/measured`. Constraints: CONSTITUTION §3.1 (local export W1, write_root), §3 (handles), §8.

## Open questions

- The parent's REQ-CORE.PRJ.TPL-006 reads "saving writes only into an approved export root"; for a human in the GUI, the picker produces a handle into a root, so this holds for every principal.

## Children

_Leaf: none._
