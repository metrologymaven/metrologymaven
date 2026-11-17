---
id: VIZ.VIEW.TRANSP
title: Per-entity and per-group transparency
level: 3
parent: VIZ.VIEW
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-VIZ.VIEW-005]
covers: [GOM.VIZ.03]
depends_on: [VIZ.VIEW.STATE, CORE]
side_effect: W2
determinism: D0
cli: [fid view set-transparency]
mcp: [view_set_transparency]
journal: [view.set_transparency@1]
updated: 2026-11-16
---

# VIZ.VIEW.TRANSP: Per-entity and per-group transparency

## Purpose

Set opacity for individual entities or groups (by entity IDs or a semantic query) in a viewport's ViewState through one journaled command.

## Scope

**In:** `view.set_transparency@1`, transparency entries in ViewState, group targeting by query.
**Out:** order-independent transparency rendering (`VIZ.RENDER.SHADE`); visibility (`VIZ.VIEW.VISIB`).

## Requirements

- **REQ-VIZ.VIEW.TRANSP-001** `view.set_transparency@1` shall set an opacity between 0.0 and 1.0 inclusive, stored as a decimal string with at most 3 decimals, for a list of entity IDs or a semantic query in a target viewport.
- **REQ-VIZ.VIEW.TRANSP-002** Every successful change shall be journaled with the resulting ViewState digest.
- **REQ-VIZ.VIEW.TRANSP-003** If the opacity is outside 0 to 1 or has more than 3 decimals, then the command shall fail with a typed error and journal nothing.
- **REQ-VIZ.VIEW.TRANSP-004** Setting opacity 1.0 shall remove the entry so that fully opaque is the default state.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/golden-cube`, when `fid view set-transparency --viewport main --entities <cube-id> --opacity 0.4 --json` runs, then the ViewState holds one entry with opacity "0.4" and the journal holds one entry with the digest.
- **AC-2** Given opacity 1.0 afterwards, then the entry is removed.
- **AC-3** (replay) Given the journal, when replayed in exact mode, then the outcome is `IDENTICAL`.
- **AC-4** (error path) Given opacity 1.5, then it fails with `OpacityOutOfRange` and journals nothing.

## Design

- **Registry command(s):** `view.set_transparency@1`, W2. Parameters: `viewport`, `target: Entities([EntityId]) | Query(SemanticQuery)`, `opacity: Decimal3`. Result: `{ view_state_digest }`.
- **Algorithm:** validate; upsert or remove entries keyed by target; recompute digest.
- **Errors:** `OpacityOutOfRange`, `UnknownViewport`, `InvalidQuery`.
- **Determinism:** D0.
- **Performance budget:** under 5 ms for 10 000 entities.
- **Crate:** `fid-view` (module `transp`).

## Test fixtures

`fixtures/viz/golden-cube`, in-repo.

## Build prompt

> In crate `fid-view`, add module `transp` and registry command `view.set_transparency@1` (W2; CLI `fid view set-transparency`, MCP `view_set_transparency`). Target either a list of entity IDs or a semantic query; opacity is a decimal string 0–1 with at most 3 decimals; opacity 1.0 removes the entry. Update the viewport's `ViewState` and journal the resulting digest. Reject out-of-range or over-precise values with `OpacityOutOfRange` without journaling. Tests: set, remove at 1.0, exact replay `IDENTICAL`, error. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- None blocking.

## Children

_Leaf: none._
