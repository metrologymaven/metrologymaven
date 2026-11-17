---
id: VIZ.VIEW.VISIB.MODEL
title: Visibility rules, layers, explorer filters and precedence
level: 4
parent: VIZ.VIEW.VISIB
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.VIEW.VISIB-002, REQ-VIZ.VIEW.VISIB-004, REQ-VIZ.VIEW.VISIB-005]
covers: [GOM.VIZ.06]
depends_on: [VIZ.VIEW.STATE, GEO.SEL.MODEL, GEO.SEL.QUERY, CORE.PRJ.TREE]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.VIEW.VISIB.MODEL: Visibility rules, layers, explorer filters and precedence

## Purpose

The pure evaluation of "what is visible": explicit per-entity rules, explorer filters stored as GEO semantic queries (type, status, tag, name pattern, spatial) and layers, combined in a fixed precedence (explicit entity rule over filter over layer) into a visible-entity set. The same rules on the same document state give the same set from any surface.

## Scope

**In:** `VisibilityRule`, `ExplorerFilter`, `Layer` types inside ViewState; precedence evaluation; status filter values including `stale` and `not_evaluated`; bounded name-pattern matching over untrusted names.
**Out:** the command (`VIZ.VIEW.VISIB.CMD`); query grammar and resolution (`GEO.SEL.QUERY`); element-tree toggles (`VIZ.SHELL.TREE.ELTREE`); selection (`VIZ.PICK`).

## Requirements

- **REQ-VIZ.VIEW.VISIB.MODEL-001** Explorer filters shall be stored as GEO semantic queries and never as lists of render IDs.
- **REQ-VIZ.VIEW.VISIB.MODEL-002** Visibility of an entity shall be decided by the first match in the order: explicit entity rule, then the last-added matching filter, then layer visibility, then visible by default.
- **REQ-VIZ.VIEW.VISIB.MODEL-003** Status filters shall match the kernel-supplied display status values `CONFORMS`, `NONCONFORMS`, `INDETERMINATE`, `NOT_DECIDABLE`, `not_evaluated` and `stale`.
- **REQ-VIZ.VIEW.VISIB.MODEL-004** Name-pattern filters shall be glob patterns of at most 256 characters, matched in linear time without backtracking, and longer patterns shall be rejected with `PatternTooLong`.
- **REQ-VIZ.VIEW.VISIB.MODEL-005** Evaluating the same ViewState against the same document state shall return the same visible-entity set in ascending entity-ID order.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/visib-cases` (40 cases of rules, filters and layers over `scene-doc-small` with expected visible sets), when each is evaluated, then the result equals the expected set.
- **AC-2** Given an entity hidden by a layer but explicitly shown, then it is visible; given a filter hiding NONCONFORMS and a stale characteristic, then the stale one is unaffected unless a `stale` filter is set.
- **AC-3** (error path) Given a 300-character pattern, then `PatternTooLong`; given pattern `a*a*a*a*a*b` over a 10^4-character name, then matching completes under 1 ms.
- **AC-4** Given a ViewState with filters, then its serialised form contains queries and no render IDs.

## Design

- **Registry command(s):** none; library.
- **Data structures:** `VisibilityRule { entity: EntityId, visible: bool }`; `ExplorerFilter { id, query: SemanticQuery, action: Show | Hide | Only }`; `Layer { id, name: Untrusted<String>, members: SemanticQuery | Vec<EntityId>, visible: bool }`.
- **Algorithm:** resolve filter and layer queries through `GEO.SEL.QUERY` on the CPU; per entity apply precedence; glob by a two-pointer linear matcher.
- **Errors:** `PatternTooLong`, `QueryInvalid`.
- **Determinism:** D0.
- **Performance budget:** evaluate 10^5 entities with 20 filters under 20 ms [unverified].
- **Crate:** `fid-view` (module `visib::model`).

## Test fixtures

`fixtures/viz/visib-cases`: project-authored over `scene-doc-small`.

## Build prompt

> In crate `fid-view`, add `visib::model`: types `VisibilityRule`, `ExplorerFilter` (GEO semantic query plus Show/Hide/Only) and `Layer` (query or entity list) stored in ViewState; evaluate visibility with precedence explicit rule > last-added matching filter > layer > default visible; status filters match kernel display statuses including `stale` and `not_evaluated`; glob name patterns ≤ 256 chars with a linear-time matcher (`PatternTooLong` otherwise). Return visible entity IDs ascending. Never store render IDs. Tests AC-1..AC-4 with `fixtures/viz/visib-cases`. Rules: CONSTITUTION §8; C6 (entity names are untrusted); no std HashMap.

## Open questions

- None.

## Children

_Leaf: none._
