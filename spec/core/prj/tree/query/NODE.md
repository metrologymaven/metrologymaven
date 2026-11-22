---
id: CORE.PRJ.TREE.QUERY
title: List and describe queries at scale
level: 4
parent: CORE.PRJ.TREE
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-project, fid-project-cmd]
built_pr: 498
satisfies: [REQ-CORE.PRJ.TREE-002, REQ-CORE.PRJ.TREE-006]
covers: [GOM.PRJ.03, PW.PRJ.03]
depends_on: [CORE.PRJ.TREE.MODEL, CORE.BUS.PIPE.HANDLER, CORE.REG.DEF]
side_effect: [R0, R0]
determinism: D0
cli: [fid project list-elements, fid project describe-element]
mcp: [project_list_elements, project_describe_element]
journal: [project.list_elements@1, project.describe_element@1]
updated: 2026-11-17
---

# CORE.PRJ.TREE.QUERY: List and describe queries at scale

## Purpose

The read side of the element tree that every surface and the element explorer use: list elements with filters by type, state, group and stage, with paging, and describe one element in full. Both are R0 and meet a 100,000-element budget.

## Scope

**In:** `project.list_elements@1` and `project.describe_element@1`; filters, sort order and paging; result schemas; the 100,000-element benchmark.
**Out:** the element model (`CORE.PRJ.TREE.MODEL`); find by tag (`CORE.PRJ.META`); dependency queries (`CORE.DEP.EDGE`); explorer UI (`VIZ`).

## Requirements

- **REQ-CORE.PRJ.TREE.QUERY-001** When `project.list_elements@1` runs, the system shall return elements filtered by any combination of type, state, group (direct or recursive) and stage, sorted by tree order then ID, in pages of at most 5,000 with a continuation token.
- **REQ-CORE.PRJ.TREE.QUERY-002** When `project.describe_element@1` runs for an ID, the system shall return its type, parent group, path of group names, name, per-stage slots, visibility, tags and metadata, with all user text marked untrusted.
- **REQ-CORE.PRJ.TREE.QUERY-003** Listing all elements of a 100,000-element project through every page shall complete in under 500 ms on reference hardware [unverified until measured].
- **REQ-CORE.PRJ.TREE.QUERY-004** Both commands shall be side-effect class R0 and shall write nothing to the Record Journal.
- **REQ-CORE.PRJ.TREE.QUERY-005** If the ID does not exist, then `project.describe_element@1` shall return `not_found`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/prj/tree/query/p1000` (1,000 elements, 4 types, mixed states, 10 groups), when `fid project list-elements --type test.plane --state outdated --json` runs, then exactly the matching elements are returned in tree order, and the result validates against `element-list.schema.json`.
- **AC-2** Given benchmark `bench_list_100k`, then paging through all 100,000 elements takes under 500 ms [unverified until measured].
- **AC-3** Given `fid project describe-element <e7> --json`, then all fields are present and user text carries the untrusted marker.
- **AC-4** (error path) Given an unknown ID, then `not_found`; given a bad page token, then `validation`.
- **AC-5** Given no recording session, when both commands run, then the Record Journal and Activity Log are unchanged.

## Design

- **Registry command(s):**
  - `project.list_elements@1` R0; params `{ type?, state?, group?, recursive?: bool, stage?, page_token? }`; result `{ elements: [ElementSummary], next? }`.
  - `project.describe_element@1` R0; params `{ id: EntityRef }`; result `ElementDetail`.
- **Data structures:** tree-order index (pre-order of groups then ID) maintained on apply; `PageToken` encoding the last position and the head it was issued for.
- **Algorithm:** iterate the tree-order index with filters; a token from an older head is rejected with `validation` reason `stale_token`.
- **Errors:** `not_found`, `validation` (`bad_token`, `stale_token`).
- **Determinism:** D0.
- **Performance budget:** REQ-003.
- **Crate:** `fid-project` (module `query`).

## Test fixtures

`fixtures/core/prj/tree/query/p1000` and the `bench_list_100k` generator, committed with fixed seeds; CC-BY-4.0.

## Build prompt

> In crate `fid-project`, add module `query` and register `project.list_elements@1` (R0; filters type, state, group with optional recursion, stage; tree order then ID; pages ≤5,000 with a token tied to the head, `stale_token` if the head moved) and `project.describe_element@1` (R0; type, parent, group path, name, per-stage slots, visibility, tags, metadata; user text marked untrusted; `not_found`). CLI `fid project list-elements | describe-element`, MCP `project_list_elements | project_describe_element`. Maintain a tree-order index on apply. Add `bench_list_100k` (< 500 ms for all pages). Neither command writes the Record Journal. Tests: AC-1..AC-5 with `fixtures/core/prj/tree/query/p1000`. Constraints: CONSTITUTION §3 Reads (D-031), C6, §8.

## Open questions

- None.

## Children

_Leaf: none._
