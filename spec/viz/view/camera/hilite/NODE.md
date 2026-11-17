---
id: VIZ.VIEW.CAMERA.HILITE
title: Highlight and proposal ghost overlays
level: 4
parent: VIZ.VIEW.CAMERA
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.VIEW.CAMERA-004]
covers: []
depends_on: [VIZ.RENDER.SCENE.IDTAB, CORE.BUS.PRIN, CORE.REG.DEF]
side_effect: R1
determinism: D0
cli: [fid view highlight]
mcp: [view_highlight]
journal: [view.highlight@1]
updated: 2026-11-17
---

# VIZ.VIEW.CAMERA.HILITE: Highlight and proposal ghost overlays

## Purpose

Point at things without changing them: `view.highlight@1` emphasises existing entities, or shows ghost overlays of agent-proposed datums or alignments, as a transient presentation layer. Highlights from an agent are always labelled "proposal", never alter document state, and expire or clear on demand.

## Scope

**In:** `view.highlight@1` (R1); highlight sets per viewport; ghost overlays for proposal entities on an agent branch (derived IDs with view operation `Ghost`); proposal labels; clear and expiry.
**Out:** drawing ghosts (`VIZ.RENDER`); adopting proposals (`VIZ.INBOX.ACTIONS`); camera moves (`VIZ.VIEW.CAMERA.SET`).

## Requirements

- **REQ-VIZ.VIEW.CAMERA.HILITE-001** `view.highlight@1` shall set or clear a viewport's highlight set of entity references and return the resolved entity IDs and any unresolved references.
- **REQ-VIZ.VIEW.CAMERA.HILITE-002** When the principal is an agent, every highlight and ghost it creates shall carry a visible "proposal" label naming the agent principal.
- **REQ-VIZ.VIEW.CAMERA.HILITE-003** A ghost of an entity on an agent branch shall be drawn through a derived ID with view operation `Ghost` and shall not appear in the working document's element tree or results.
- **REQ-VIZ.VIEW.CAMERA.HILITE-004** `view.highlight@1` shall change no document state, and the working document digest shall be identical before and after.
- **REQ-VIZ.VIEW.CAMERA.HILITE-005** A highlight shall clear on an explicit clear, on its optional expiry in seconds, or when its viewport closes.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/scene-doc-small`, when `fid view highlight --viewport main --entities <c1>,<c2> --json` runs, then the result lists both IDs and the pixel probe shows the highlight style on both.
- **AC-2** Given an agent branch with a proposed datum, when an agent calls `view_highlight` with `ghost: <datum-id>`, then the ghost is drawn with a proposal label naming the agent, and the working document digest is unchanged.
- **AC-3** (error path) Given an unknown entity, then the result lists it as unresolved and the other highlights still apply.
- **AC-4** Given `expiry_s: 2`, then 2 s later the highlight set is empty.

## Design

- **Registry command(s):** `view.highlight@1`, R1, MCP exposed (Activity Log during recording only, D-031); params `{ viewport: ViewportId, entities: [EntityRef], ghosts: [{ branch: BranchRef, entity: EntityRef }], mode: Set | Add | Clear, expiry_s?: u32 }`; result `{ resolved: [EntityId], unresolved: [EntityRef] }`.
- **Data structures:** `HighlightSet { entries, origin: Principal, expires_at }` per viewport (presentation state).
- **Algorithm:** resolve references; build derived `Ghost` IDs for branch entities via IDTAB; attach proposal label when principal kind is agent.
- **Errors:** `UnknownViewport`, `UnknownBranch`.
- **Determinism:** D0 (resolution result).
- **Performance budget:** under 5 ms for 1 000 entities.
- **Crate:** `fid-view` (module `hilite`).

## Test fixtures

`fixtures/viz/scene-doc-small` plus an agent-branch fixture with one proposed datum.

## Build prompt

> In crate `fid-view`, add module `hilite` with registry command `view.highlight@1` (R1; CLI `fid view highlight`; MCP `view_highlight`): set/add/clear a viewport's highlight set of entity references and ghost overlays of entities on an agent branch (derived IDs with view operation `Ghost`), with optional expiry; when the principal is an agent every highlight and ghost shows a "proposal" label naming it; return resolved and unresolved references; never change document state. Tests AC-1..AC-4. Rules: CONSTITUTION §3, §8; C5 (agent work is labelled and needs human adoption elsewhere).

## Open questions

- None.

## Children

_Leaf: none._
