---
id: RPT.MODEL.BLOCKS.VIEW
title: Restorable 3D view block
level: 4
parent: RPT.MODEL.BLOCKS
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-RPT.MODEL.BLOCKS-001, REQ-RPT.MODEL.BLOCKS-005, REQ-RPT.MODEL.BLOCKS-006, REQ-RPT.MODEL.BLOCKS-012]
covers: [GOM.RPT.03, PW.RPT.03]
depends_on: [RPT.MODEL.DOC, RPT.MODEL.BIND.RESOLVE, CORE.STO.CAS]
uses_artifacts: [VIZ.VIEW.NAMED, VIZ.CAPTURE.SHOT]
side_effect: W2
determinism: D0
cli: [fid report set-view-block]
mcp: [report_set_view_block]
journal: [report.set_view_block@1]
updated: 2026-11-19
---

# RPT.MODEL.BLOCKS.VIEW: Restorable 3D view block

## Purpose

The `view` block kind: a snapshot of the 3D scene in a report that can be reopened as the live scene. The block stores the NamedView ID and digest (published by `VIZ.VIEW.NAMED`) and the snapshot PNG digest with its capture manifest (published by `VIZ.CAPTURE.SHOT`). When a user opens the block, the app passes the stored NamedView ID to `view.restore_view@1`; RPT never renders or restores scenes itself, so it needs no code edge to VIZ (an L3 surface).

## Scope

**In:** `view` payload schema v1; `report.set_view_block@1` (W2) that records a NamedView ID and a snapshot digest after checking both exist in the project store; `BlockResolver` implementation; the `report.view_missing` warning; an `open_target(block) -> NamedViewId` query the GUI uses to restore.

**Out:** taking snapshots and saving views (`VIZ.CAPTURE.SHOT`, `VIZ.VIEW.NAMED`); the restore action itself (VIZ, which calls `view.restore_view@1`); embedding bytes in PDF (`RPT.OUTPUT.PDF.DRAW`).

## Requirements

- **REQ-RPT.MODEL.BLOCKS.VIEW-001** The `view` kind shall register payload schema version 1 holding the NamedView ID and digest, the snapshot PNG digest and its capture-manifest digest, and an untrusted caption of at most 256 bytes.
- **REQ-RPT.MODEL.BLOCKS.VIEW-002** When `report.set_view_block@1` runs, the system shall check that the NamedView record and the snapshot bytes exist in the project's content-addressed store, store their digests and journal one event; otherwise it shall fail with error `report.view_not_found`.
- **REQ-RPT.MODEL.BLOCKS.VIEW-003** When the GUI asks for the open target of a `view` block, the system shall return the stored NamedView ID so the scene can be restored to that saved view.
- **REQ-RPT.MODEL.BLOCKS.VIEW-004** If the NamedView no longer exists at generation, then the resolved block shall keep the stored snapshot digest and generation shall report warning `report.view_missing` naming the block.
- **REQ-RPT.MODEL.BLOCKS.VIEW-005** If the snapshot bytes are missing from the store at generation, then generation shall fail with error `report.snapshot_missing` naming the digest.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/model/bracket_views` (two NamedViews with snapshots), when `fid report set-view-block --view V1 --snapshot <d1> --json` runs, then the payload holds V1's ID and digest and the snapshot digests, and one event is journaled.
- **AC-2** Given AC-1, when the open target is queried, then it returns V1's ID.
- **AC-3** Given AC-1 and V1 deleted afterwards, when generated, then the resolved block still holds `<d1>` and warning `report.view_missing` is listed.
- **AC-4** (error path) Given a random NamedView ID, when set, then it fails with `report.view_not_found`; given a snapshot digest whose bytes were removed, generation fails with `report.snapshot_missing`.
- **AC-5** (replay) Given AC-1's journal, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command:** `report.set_view_block@1(report, block, view: NamedViewId, snapshot: Sha256, caption: Option<Text>)` (W2).
- **Data structures:** `ViewPayload { view: NamedViewId, view_digest: Sha256, snapshot: Sha256, manifest: Sha256, caption }`; `open_target(&Block) -> Option<NamedViewId>`.
- **Algorithm:** existence checks read the NamedView and capture-manifest records as published data (format owned by VIZ) from `CORE.STO.CAS`; nothing is decoded beyond their IDs and digests.
- **Errors:** `report.view_not_found`, `report.snapshot_missing`, `report.invalid_block_payload`.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-report`.

## Test fixtures

`fixtures/rpt/model/bracket_views`: a project with two NamedViews and PNG snapshots produced by the VIZ headless renderer test builder; CC-BY-4.0.

## Build prompt

> In crate `fid-report`, add the `view` block kind (payload v1: NamedView ID and digest, snapshot PNG digest, capture-manifest digest, untrusted caption up to 256 bytes), the command `report.set_view_block@1` (W2; checks both records exist in the content-addressed store, else `report.view_not_found`; CLI `fid report set-view-block --json`), an `open_target` query returning the NamedView ID for the GUI's restore action, and a `BlockResolver` that keeps the snapshot digest, warns `report.view_missing` when the NamedView is gone and fails `report.snapshot_missing` when bytes are gone. Read VIZ records as data only; do not link VIZ crates (RPT is L2, VIZ L3; CONSTITUTION §2.1). Acceptance: AC-1..AC-5 in `spec/rpt/model/blocks/view/NODE.md`. Constraints: CONSTITUTION C2 (no GPU output is a value of record), C6, §8.

## Open questions

- The NamedView and capture-manifest record formats must be listed with schema versions in the Interfaces of `VIZ.VIEW.NAMED` and `VIZ.CAPTURE.SHOT` (publisher side of D-034); `VIZ.VIEW.NAMED` is still open.

## Children

_Leaf: none._
