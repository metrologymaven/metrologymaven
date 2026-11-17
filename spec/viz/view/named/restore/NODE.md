---
id: VIZ.VIEW.NAMED.RESTORE
title: Exact restore with missing-reference report and headless check
level: 4
parent: VIZ.VIEW.NAMED
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.VIEW.NAMED-002, REQ-VIZ.VIEW.NAMED-003, REQ-VIZ.VIEW.NAMED-004, REQ-VIZ.VIEW.NAMED-005]
covers: [GOM.VIZ.01]
depends_on: [VIZ.VIEW.NAMED.STORE, VIZ.RENDER.HEADLESS, CORE.REG.DEF]
side_effect: W2
determinism: D0
cli: [fid view restore-view]
mcp: [view_restore_view]
journal: [view.restore_view@1]
updated: 2026-11-17
---

# VIZ.VIEW.NAMED.RESTORE: Exact restore with missing-reference report and headless check

## Purpose

Bring a saved view back exactly: on the same document state the restored ViewState's canonical bytes equal the saved bytes; when entities, colour maps or label sets have gone, the rest is applied and every missing reference is reported; and a headless restore check proves the image still matches the one rendered at save time.

## Scope

**In:** `view.restore_view@1` (W2); reference resolution against the current document; missing-reference report; headless restore check used by CI and by RPT before placing a view.
**Out:** saving and listing (`VIZ.VIEW.NAMED.STORE`); rendering (`VIZ.RENDER.HEADLESS`); image golden tolerance definitions (`VIZ.RENDER.ORACLE.GATE`).

## Requirements

- **REQ-VIZ.VIEW.NAMED.RESTORE-001** When a NamedView is restored on the same document state, the target viewport's ViewState canonical bytes shall equal the saved bytes.
- **REQ-VIZ.VIEW.NAMED.RESTORE-002** If the NamedView references entities, colour maps or label sets that no longer exist, then restore shall apply the remaining state, drop only the unresolved references, and return each missing reference with its kind and ID in sorted order.
- **REQ-VIZ.VIEW.NAMED.RESTORE-003** Every restore shall be journaled with the NamedView ID, the restored ViewState digest and the count of missing references.
- **REQ-VIZ.VIEW.NAMED.RESTORE-004** The headless restore check shall render the restored view and compare it with the image stored at save time within the golden-image tolerance mask, reporting pass or the failing-pixel count.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/named-views` (5 saved views on `scene-doc-small`), when each is restored with `fid view restore-view --view <id> --json`, then each result digest equals the saved digest.
- **AC-2** Given one characteristic deleted after saving, when its view is restored, then the result lists that entity under `missing` and the other state is applied.
- **AC-3** Given the headless check on all 5 views on the software adapter, then all pass within the mask.
- **AC-4** (error path) Given an unknown view ID, then restore fails with `UnknownView` and journals nothing.
- **AC-5** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `view.restore_view@1`, W2; params `{ view_id, viewport }`; result `{ view_state_digest, missing: [{ kind: Entity | ColorMap | LabelSet, id }] }`. Headless check is a library function `check_restore(view_id) -> RestoreCheck` used by tests, CI and `RPT`.
- **Algorithm:** parse bytes; resolve each reference; build the reduced ViewState; when nothing is missing, assert byte equality; write ViewState; render headless and compare with the stored save-time image digest and mask.
- **Errors:** `UnknownView`, `UnknownViewport`, `UnsupportedSchema`.
- **Determinism:** D0 (state); image comparison is per adapter within mask.
- **Performance budget:** restore under 10 ms; headless check under 1 s at 1280 × 720 on the software adapter [unverified].
- **Crate:** `fid-view` (module `named::restore`).

## Test fixtures

`fixtures/viz/named-views`: 5 views over `fixtures/viz/scene-doc-small` with save-time images and masks, project-authored.

## Build prompt

> In crate `fid-view`, add `named::restore` with registry command `view.restore_view@1` (W2; CLI `fid view restore-view`; MCP `view_restore_view`): parse the saved canonical bytes, resolve entity, colour-map and label-set references, apply the remaining state, return the sorted list of missing references, journal the NamedView ID, the restored digest and the missing count; on an unchanged document the restored bytes must equal the saved bytes. Provide `check_restore` that renders headlessly and compares with the save-time image within the golden mask. Tests AC-1..AC-5 with `fixtures/viz/named-views`. Rules: CONSTITUTION §3, §8.

## Open questions

- None.

## Children

_Leaf: none._
