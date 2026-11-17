---
id: VIZ.VIEW.MULTI
title: Multi-viewport layouts and camera sync
level: 3
parent: VIZ.VIEW
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.VIEW-001, REQ-VIZ.VIEW-002]
covers: [PW.VIZ.02]
depends_on: [VIZ.VIEW.STATE, VIZ.VIEW.CAMERA.NAV, CORE.JRN.BR, CORE.REG.DEF]
side_effect: W2
determinism: D0
cli: [fid view set-viewports]
mcp: [view_set_viewports]
journal: [view.set_viewports@1]
updated: 2026-11-17
---

# VIZ.VIEW.MULTI: Multi-viewport layouts and camera sync

## Purpose

Several viewports in one view: split layouts, cameras synced or independent per viewport, and before/after split views (for example a proposal versus the working document, or two alignments). The layout and its per-viewport ViewStates are part of what a NamedView saves and restores, and the layout changes through one journaled command.

## Scope

**In:**
- `ViewportLayout` model (single, two-way vertical or horizontal, four-way, custom splits as a binary tree of ratios) embedded in the saved view state
- Camera sync groups (full, rotation only, none)
- Before/after split: each viewport bound to a document state (working document, a branch head or a journal entry hash), with a side label
- `view.set_viewports@1` (W2) to change layout, sync groups and bindings
- Split-view API used by `VIZ.INBOX.DIFF`

**Out** (and which node owns it instead):
- Window docking and panels (`VIZ.SHELL.DOCK`)
- Proposal diff content (`VIZ.INBOX.DIFF`)
- Camera commands and gestures (`VIZ.VIEW.CAMERA`)
- ViewState canonical form (`VIZ.VIEW.STATE`)

## Requirements

- **REQ-VIZ.VIEW.MULTI-001** The viewport layout and every viewport's ViewState shall be captured in the saved view state and restored byte-identically.
- **REQ-VIZ.VIEW.MULTI-002** While cameras are synced, a camera change in one viewport shall apply to every viewport in the sync group in the same frame.
- **REQ-VIZ.VIEW.MULTI-003** A before/after split shall label each side with the document state or branch head it shows.
- **REQ-VIZ.VIEW.MULTI-004** The system shall support at least 4 viewports per layout [unverified].
- **REQ-VIZ.VIEW.MULTI-005** Every successful `view.set_viewports@1` shall be journaled with the resulting layout digest, and if a split ratio lies outside 0.1 to 0.9, a sync group names an unknown viewport, or more than 8 viewports are requested, then the command shall fail with a typed error and journal nothing.
- **REQ-VIZ.VIEW.MULTI-006** In rotation-only sync, viewports shall share view direction and up vector but keep their own target and distance.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/golden-cube`, when `fid view set-viewports --layout quad --sync full --json` runs, then the result holds 4 viewports and its digest equals the journal entry's digest.
- **AC-2** Given the quad layout saved and restored through `VIZ.VIEW.NAMED`, then the canonical bytes of the restored layout equal the saved bytes.
- **AC-3** Given full sync, when viewport 1 is orbited, then in the same frame all four cameras are bit-identical; given rotation-only sync, then directions match and targets differ as before.
- **AC-4** Given a two-way split bound to the working document and an agent branch head, then each side shows its label with the branch name and head hash.
- **AC-5** (error path) Given a split ratio of 0.95, then the command fails with `RatioOutOfRange` and journals nothing.
- **AC-6** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `view.set_viewports@1`, W2 (agents write to their branch); params `{ layout: Single | Split { dir: H | V, ratio: Decimal, a: Layout, b: Layout } | Quad, sync: [{ group: [ViewportId], mode: Full | Rotation }], bindings: [{ viewport: ViewportId, source: Working | Branch(BranchRef) | Entry(Hash) }] }`; result `{ layout_digest, viewports: [ViewportId] }`.
- **Data structures:** `ViewportLayout { tree, viewports: Vec<(ViewportId, ViewState)>, sync, bindings }`, serialised canonically (RFC 8785) next to ViewState under the same schema version rules.
- **Algorithm:** validate; assign viewport IDs deterministically (layout-tree order); on camera change in a synced viewport, propagate within the same frame before rendering.
- **Errors:** `RatioOutOfRange`, `UnknownViewport`, `TooManyViewports`, `UnknownBranch`.
- **Determinism:** D0.
- **Performance budget:** command under 5 ms; sync propagation under 0.1 ms.
- **Crate:** `fid-view` (module `multi`).

## Test fixtures

`fixtures/viz/golden-cube`; an agent-branch fixture for AC-4.

## Build prompt

> In crate `fid-view`, add module `multi`: a `ViewportLayout` (single, split tree with decimal ratios in 0.1–0.9, quad; up to 8 viewports) holding per-viewport ViewStates, sync groups (full or rotation-only) and per-viewport document bindings (working, branch head, journal entry), all serialised canonically with the ViewState rules. Add registry command `view.set_viewports@1` (W2; CLI `fid view set-viewports`; MCP `view_set_viewports`) that validates (typed errors, journal nothing on failure), applies and journals the layout digest. Synced camera changes propagate in the same frame; before/after splits label each side with branch name and head hash. Tests AC-1..AC-6 with `fixtures/viz/golden-cube`. Rules: CONSTITUTION §3, §4 (decimal ratios keep their authored value), §8.

## Open questions

- Tagged P1b: P1a ships a single viewport; the Inbox visual diff in P1a uses a toggle rather than a split [unverified].
- `view.set_viewports@1` is a new command name not listed in the VIZ.VIEW frontmatter; the parent's `view.*` pattern covers it.

## Children

_Leaf: none._
