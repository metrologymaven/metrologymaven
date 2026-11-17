---
id: VIZ.VIEW.VISIB
title: Visibility, layers and explorer filters
level: 3
parent: VIZ.VIEW
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.VIEW-005, REQ-VIZ.VIEW-009]
covers: [GOM.VIZ.06]
depends_on: [VIZ.VIEW.STATE, GEO.SEL, CORE]
cli: [fid view set-visibility]
mcp: [view_set_visibility]
journal: [view.set_visibility@*]
updated: 2026-11-17
---

# VIZ.VIEW.VISIB: Visibility, layers and explorer filters

## Purpose

What is shown: explicit show and hide per entity, layers, and explorer filters by type, status, tag or query. Filters are semantic queries, so the same filter selects the same entities from the GUI, CLI and MCP and survives replay.

## Scope

**In:**
- `view.set_visibility@1` for entities, layers and filters
- Explorer filter model as GEO semantic queries (type, status, tag, name pattern, spatial)
- Layer definitions and their membership
- Show-only, isolate and show-all operations

**Out** (and which node owns it instead):
- Element-tree UI that toggles visibility (`VIZ.SHELL`)
- Selection (`VIZ.PICK`)
- Query types (`GEO.SEL`)

## Requirements

- **REQ-VIZ.VIEW.VISIB-001** Every visibility change on the working document shall be performed by `view.set_visibility@1` and journaled with the resulting ViewState digest.
- **REQ-VIZ.VIEW.VISIB-002** Explorer filters shall be stored as semantic queries, never as lists of render IDs.
- **REQ-VIZ.VIEW.VISIB-003** The same filter evaluated from GUI, CLI and MCP on the same document state shall select the same entity set.
- **REQ-VIZ.VIEW.VISIB-004** Status filters shall use the kernel-supplied decision outcome, including stale and not evaluated.
- **REQ-VIZ.VIEW.VISIB-005** Visibility rules shall apply in a defined precedence: explicit entity rule over filter over layer.

## Interfaces

- **Offers:** `view.set_visibility@1`; `VisibilityRule` and `ExplorerFilter` types.
- **Consumes:** `GEO.SEL` queries; entity statuses from CORE/GDT.

## Decomposition hints

RULES (model and precedence), FILTER (explorer queries), LAYER, CMD.

## Open questions

- Name-pattern filters over untrusted entity names must be bounded (pattern size and time) to stay fail-closed.
- Applied: name patterns are bounded to 256 characters with a linear-time matcher (MODEL REQ-004).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| VIZ.VIEW.VISIB.MODEL | Visibility rules, layers, explorer filters and precedence (leaf) | 002 004 005 | GOM.VIZ.06 |
| VIZ.VIEW.VISIB.CMD | Set-visibility command with isolate, show-only and layers (leaf) | 001 003 | GOM.VIZ.06 |
