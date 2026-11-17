---
id: CORE.PRJ.TREE
title: Element tree, element states and type registration
level: 3
parent: CORE.PRJ
kind: module
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-CORE.PRJ-006, REQ-CORE.PRJ-007, REQ-CORE.PRJ-008, REQ-CORE.PRJ-011]
covers: [GOM.PRJ.03, PW.PRJ.03]
depends_on: [CORE.BUS.PIPE, CORE.PRJ.CONT]
cli: [fid project list-elements, fid project describe-element, fid project create-group, fid project move-element, fid project rename-element, fid project set-visibility]
mcp: [project_list_elements, project_describe_element, project_create_group, project_move_element, project_rename_element, project_set_visibility]
journal: [project.create_group@1, project.move_element@1, project.rename_element@1, project.set_visibility@1]
updated: 2026-11-17
---

# CORE.PRJ.TREE: Element tree, element states and type registration

## Purpose

Holds every element of a project in one tree: its registered type, its parent group, its state (`current | outdated | failed | not_computed`), its name, visibility flag and metadata slot. Other domains add element types only through the registration API here. The tree is what the element explorer (VIZ) shows and what the dependency graph (DEP) walks.

## Scope

**In:**
- Element record: ID, type, parent group, name, state, visibility (document state), metadata slot
- Groups and hierarchy; move, rename, create group
- Element states and the state transition rules other modules use
- `ElementTypeRegistry`: registration by domains, rejection of unregistered types
- Queries: list and describe elements with filters (type, state, group)
- Scale budget (100,000 elements)

**Out** (and which node owns it instead):
- Tags and key-value metadata commands (owned by `CORE.PRJ.META`)
- Setting states as a result of change propagation and recompute (owned by `CORE.DEP`)
- Element explorer UI (owned by `VIZ`)
- Element payload types (owned by `GEO`, `ALN`, `GDT`, `SCAN`)

## Requirements

- **REQ-CORE.PRJ.TREE-001** The element tree shall hold every element of a project with its ID, registered type, parent group, name, state, visibility flag and metadata slot.
- **REQ-CORE.PRJ.TREE-002** The element tree shall support at least 100,000 elements, with `project.list_elements@1` over all of them returning in under 500 ms on reference hardware [unverified until measured].
- **REQ-CORE.PRJ.TREE-003** Every element shall carry exactly one state from `current`, `outdated`, `failed` or `not_computed`.
- **REQ-CORE.PRJ.TREE-004** If an element's type is not registered in the `ElementTypeRegistry`, then the element tree shall reject its creation with a typed `unregistered_type` error.
- **REQ-CORE.PRJ.TREE-005** If a move would make a group its own ancestor, then the element tree shall reject it with a typed `hierarchy` error.
- **REQ-CORE.PRJ.TREE-006** Grouping, moving, renaming and visibility changes shall be registry commands of side-effect class W2, and list and describe shall be R0.
- **REQ-CORE.PRJ.TREE-007** Element names shall be typed as untrusted text and shall not be required to be unique; identity shall be the element ID only.

## Interfaces

- **Offers:** `ElementTree` traversal and queries, `ElementTypeRegistry::register(type_def)`, `ElementState`.
- **Commands:** `project.list_elements@1` (R0), `project.describe_element@1` (R0), `project.create_group@1` (W2), `project.move_element@1` (W2), `project.rename_element@1` (W2), `project.set_visibility@1` (W2).
- **Consumes:** `CORE.BUS`, `CORE.PRJ.CONT`.

## Decomposition hints

MODEL (element record and states), TYPES (type registration), GROUP (groups and moves), QUERY (list and describe with filters), SCALE (100k benchmark).

## Open questions

- Visibility as document state follows the concept rule that saved views are document state; transient hide/show in the viewer stays view state in VIZ. Confirm the boundary with VIZ.VIEW.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| CORE.PRJ.TREE.MODEL | Element record, states, state view and type registry (leaf) | 001 003 004 007 | GOM.PRJ.03, PW.PRJ.03 |
| CORE.PRJ.TREE.GROUP | Groups, move, rename and visibility commands (leaf) | 005 006 007 | none |
| CORE.PRJ.TREE.QUERY | List and describe queries at scale (leaf) | 002 006 | GOM.PRJ.03, PW.PRJ.03 |
