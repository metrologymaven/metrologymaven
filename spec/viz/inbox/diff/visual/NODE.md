---
id: VIZ.INBOX.DIFF.VISUAL
title: Visual proposal diff with before/after toggle and split
level: 4
parent: VIZ.INBOX.DIFF
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.INBOX.DIFF-005]
covers: []
depends_on: [VIZ.VIEW.MULTI, VIZ.VIEW.CAMERA, VIZ.RENDER.SCENE, CORE.RPL.RECON.FOLD, CORE.JRN.BR.DIFF]
uses_artifacts: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.INBOX.DIFF.VISUAL: Visual proposal diff with before/after toggle and split

## Purpose

Shows what a proposal changes in 3D: a second scene is built from the document state at the proposal's source head (reconstructed by the replay engine, never committed), changed and added entities are drawn as labelled "proposal" ghosts over the working document, and the reviewer can toggle before/after or split the viewport into target and proposal. The command result carries a `VisualDiffDescriptor` so CLI and MCP callers get the same facts without pixels.

## Scope

**In:** `VisualDiffDescriptor { target_head, source_head, added, changed, removed, focus_bounds }`; read-only scene for the source head; ghost style and the "proposal" label; toggle and split through `VIZ.VIEW.MULTI`; fly-to the changed entities through `VIZ.VIEW.CAMERA`.
**Out:** ghost drawing primitives (`VIZ.RENDER`); split-view layouts (`VIZ.VIEW.MULTI`); state reconstruction (`CORE.RPL.RECON.FOLD`).

## Requirements

- **REQ-VIZ.INBOX.DIFF.VISUAL-001** The visual diff shall offer a before/after toggle and a side-by-side split of target head and source head.
- **REQ-VIZ.INBOX.DIFF.VISUAL-002** Every entity drawn from the source head that differs from the target shall carry the visible label "proposal" and a ghost style distinct from working-document geometry.
- **REQ-VIZ.INBOX.DIFF.VISUAL-003** The source-head scene shall be built from reconstructed state held in memory only, and building it shall commit nothing and change no working-document state.
- **REQ-VIZ.INBOX.DIFF.VISUAL-004** The descriptor's added, changed and removed lists shall equal the entity sets of `journal.diff_branch@1` for the same heads.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/inbox-resdiff`, when the visual diff opens, then the descriptor lists equal the branch diff's sets and a headless render shows the "proposal" label on each changed entity (golden image with tolerance mask).
- **AC-2** Given the toggle, then the viewport alternates between target and source states; given split, then two viewports show them with synced cameras.
- **AC-3** Given the journal and project database before and after opening, closing and toggling the diff, then both are byte-identical.

## Design

- **Registry command(s):** none; the descriptor is part of `view.show_proposal@1`.
- **Algorithm:** fold the source head into an in-memory state; build a scene graph for the changed entities only, flagged `proposal`; render it as an overlay layer (toggle) or in a second viewport (split); compute focus bounds from entity bounds.
- **Errors:** `reconstruction_failed { reason }` shows a message in place of the visual layer.
- **Determinism:** D0 for the descriptor; pixels follow VIZ.RENDER tolerances.
- **Performance budget:** visual diff ready within 2 s for a proposal changing 100 entities on the reference profile [unverified].
- **Crate:** `fid-inbox` (module `diff::visual`).

## Test fixtures

`fixtures/viz/inbox-resdiff` (shared with SEMRES), plus a golden image.

## Build prompt

> In crate `fid-inbox`, add module `diff::visual`. Build a `VisualDiffDescriptor` (heads, added, changed, removed entity IDs equal to `journal.diff_branch@1`, focus bounds) and, for the GUI, an in-memory scene of the source head's changed entities reconstructed with `CORE.RPL.RECON.FOLD` (never committed), drawn as ghosts labelled "proposal"; offer before/after toggle and side-by-side split through the multi-viewport API; fly to the changes. Tests AC-1..AC-3. Rules: no document mutation; GPU only displays (C2); no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- Requires `VIZ.VIEW.MULTI` (P1b, open) for split view; in P1a only the toggle is available unless MULTI moves earlier [unverified].

## Children

_Leaf: none._
