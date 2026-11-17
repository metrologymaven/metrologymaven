---
id: VIZ.VIEW.VISIB.CMD
title: Set-visibility command with isolate, show-only and layers
level: 4
parent: VIZ.VIEW.VISIB
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.VIEW.VISIB-001, REQ-VIZ.VIEW.VISIB-003]
covers: [GOM.VIZ.06]
depends_on: [VIZ.VIEW.VISIB.MODEL, CORE.REG.DEF]
side_effect: W2
determinism: D0
cli: [fid view set-visibility]
mcp: [view_set_visibility]
journal: [view.set_visibility@1]
updated: 2026-11-17
---

# VIZ.VIEW.VISIB.CMD: Set-visibility command with isolate, show-only and layers

## Purpose

The single command through which every visibility change happens, from the element tree, the palette, the CLI or MCP: show or hide entities, isolate, show only, show all, add or remove explorer filters, and create, edit or toggle layers. It is journaled with the resulting ViewState digest, so replay restores the same visible set.

## Scope

**In:** `view.set_visibility@1` (W2) with its operations; journaling; result with digest and visible count.
**Out:** evaluation rules (`VIZ.VIEW.VISIB.MODEL`); tree UI (`VIZ.SHELL.TREE.ELTREE`, which issues this command).

## Requirements

- **REQ-VIZ.VIEW.VISIB.CMD-001** `view.set_visibility@1` shall support the operations show, hide, isolate, show-only, show-all, add-filter, remove-filter, set-layer and toggle-layer on a target viewport.
- **REQ-VIZ.VIEW.VISIB.CMD-002** Every successful call shall be journaled with the resulting ViewState digest and shall return that digest and the visible-entity count.
- **REQ-VIZ.VIEW.VISIB.CMD-003** The same call from GUI, CLI and MCP on the same document state shall produce the same ViewState digest and the same visible set.
- **REQ-VIZ.VIEW.VISIB.CMD-004** If a call references an unknown entity, filter or layer, or an invalid query, then it shall fail with a typed error and journal nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/scene-doc-small`, when `fid view set-visibility --viewport main --op isolate --entities <c3> --json` runs, then the visible count equals 1 and the digest equals the journal entry's digest.
- **AC-2** Given the same request sent over MCP by a script principal in a test harness and through the GUI test host, then the three digests are identical.
- **AC-3** Given `--op add-filter --query 'status = NONCONFORMS' --action only`, then only nonconforming characteristics remain visible.
- **AC-4** (error path) Given an unknown layer ID, then `UnknownLayer` and no journal entry.
- **AC-5** (replay) Given the journal of AC-1 and AC-3, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `view.set_visibility@1`, W2 (agents write to their branch); params `{ viewport, op: Show { entities } | Hide { entities } | Isolate { entities } | ShowOnly { query } | ShowAll | AddFilter { query, action } | RemoveFilter { id } | SetLayer { id?, name, members, visible } | ToggleLayer { id } }`; result `{ view_state_digest, visible_count }`.
- **Algorithm:** apply op to the ViewState rules; evaluate with MODEL; digest; journal.
- **Errors:** `UnknownEntity`, `UnknownFilter`, `UnknownLayer`, `QueryInvalid`, `PatternTooLong`, `UnknownViewport`.
- **Determinism:** D0.
- **Performance budget:** under 30 ms for 10^5 entities [unverified].
- **Crate:** `fid-view` (module `visib::cmd`).

## Test fixtures

`fixtures/viz/scene-doc-small`.

## Build prompt

> In crate `fid-view`, add `visib::cmd` with registry command `view.set_visibility@1` (W2; CLI `fid view set-visibility`; MCP `view_set_visibility`): operations show, hide, isolate, show-only (query), show-all, add-filter, remove-filter, set-layer, toggle-layer on a viewport's ViewState; evaluate with `visib::model`; return and journal the ViewState digest and visible count; typed errors journal nothing. Tests AC-1..AC-5 with `fixtures/viz/scene-doc-small`. Rules: CONSTITUTION §3, §8; C3 (one command for every surface).

## Open questions

- None.

## Children

_Leaf: none._
