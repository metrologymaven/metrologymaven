---
id: CORE.REG.DEF.GUIDE
title: Task-guide declarations
level: 4
parent: CORE.REG.DEF
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-CORE.REG.DEF-010]
covers: []
depends_on: [CORE.REG.DEF.FIELD]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.REG.DEF.GUIDE: Task-guide declarations

## Purpose

Lets a domain declare, next to its commands, a **task guide**: a named, ordered list of command steps with a short purpose each, telling an agent or a person which commands to call for a task such as "align a scan to CAD". Guides hold no values, no standards text and nothing executable. `AUTO.MCP.SKILLS` turns them into Skills and `AGENT.SKILLS` curates which guides each MCP profile features.

## Scope

**In:** the `TaskGuide` type, its JSON shape and schema, the `fid_guides!` declaration macro arm, reference checks between a guide and the declarations of the same build.
**Out:** rendering Skills (`AUTO.MCP.SKILLS`); the curated workflow catalogue and content standard (`AGENT.SKILLS`); placing guides in the manifest (`CORE.REG.MAN`).

## Requirements

- **REQ-CORE.REG.DEF.GUIDE-001** A task guide shall hold an ID matching `^[a-z][a-z0-9]*\.[a-z][a-z0-9_]*$`, a title of at most 80 characters, a summary of at most 500 characters, an MCP profile name and 1 to 30 steps, each step naming a command ID and major and a purpose of at most 200 characters.
- **REQ-CORE.REG.DEF.GUIDE-002** If a guide step names a command ID and major that is not declared in the same registry build, then the registry build shall fail with `guide_unknown_command` naming the guide and step.
- **REQ-CORE.REG.DEF.GUIDE-003** If a guide step names a command of class M1, S1 or A0, then the registry build shall fail with `guide_forbidden_step`.
- **REQ-CORE.REG.DEF.GUIDE-004** The guide type shall carry no numeric value field and no free-form body beyond title, summary and step purposes.
- **REQ-CORE.REG.DEF.GUIDE-005** Guides shall serialise to JSON in declaration order of steps, and a list of guides shall be sorted by guide ID.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/reg/def/guide/guides_ok.json` (guide `demo.align_scan` with three steps over fixture commands), when loaded with the fixture declarations, then it round-trips equal and passes all checks.
- **AC-2** (error path) Given a guide naming `demo.ghost_cmd@1`, when the registry builds, then it fails with `guide_unknown_command`.
- **AC-3** (error path) Given a guide naming the M1 fixture command `demo.move_axis@1`, then the build fails with `guide_forbidden_step`.
- **AC-4** (error path) Given a guide with 31 steps or a 201-character purpose, then the build fails with `guide_too_large`.

## Design

- **Registry command(s):** none; declaration type.
- **Data structures:** `TaskGuide { id, title, summary, profile, steps: Vec<GuideStep { command: CommandRef { id, major }, purpose }> }`.
- **Algorithm:** check lengths, then look each step up in the sorted declaration list of the build.
- **Errors:** `guide_unknown_command`, `guide_forbidden_step`, `guide_too_large`, `guide_bad_id`.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-registry` (module `guide`); macro arm in `fid-registry-macros`.

## Test fixtures

`fixtures/core/reg/def/guide/`, sharing the fixture commands of `fixtures/manifest/guides.json` used by `AUTO.MCP.SKILLS`; CC-BY-4.0.

## Build prompt

> In crate `fid-registry`, add module `guide` with `TaskGuide { id, title (≤ 80), summary (≤ 500), profile, steps (1..=30) of { command: id@major, purpose (≤ 200) } }`, JSON serialisation (steps in order; guide lists sorted by ID) and `check_guides(&[TaskGuide], &[CommandDecl]) -> Vec<GuideError>` reporting `guide_unknown_command`, `guide_forbidden_step` (M1, S1, A0), `guide_too_large`, `guide_bad_id`. Add a `guides` arm to `fid_commands!` in `fid-registry-macros`. Guides carry no values and no standards text. Tests AC-1..AC-4. Constraints: CONSTITUTION C3, C6 (guide text is project-authored, not content), §8.

## Open questions

- `CORE.REG.MAN` must add a `task_guides` array (sorted by ID) to the manifest so `AUTO.MCP.SKILLS` can read it; until then the guides exist only in the build.

## Children

_Leaf: none._
