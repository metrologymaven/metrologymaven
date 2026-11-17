---
id: VIZ.VIEW.NAMED
title: NamedView lifecycle and exact restore
level: 3
parent: VIZ.VIEW
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.VIEW-001, REQ-VIZ.VIEW-002, REQ-VIZ.VIEW-003, REQ-VIZ.VIEW-004, REQ-VIZ.VIEW-005]
covers: [GOM.VIZ.01, PW.VIZ.02]
depends_on: [VIZ.VIEW.STATE, VIZ.RENDER.HEADLESS, CORE]
cli: [fid view save-view, fid view restore-view, fid view list-views, fid view delete-view]
mcp: [view_save_view, view_restore_view, view_list_views, view_delete_view]
journal: [view.save_view@*, view.restore_view@*, view.delete_view@*]
updated: 2026-11-16
---

# VIZ.VIEW.NAMED: NamedView lifecycle and exact restore

## Purpose

NamedViews: saved, content-addressed bundles of a ViewState (camera, clips, visibility, transparency, colour-map, legend and label references) with a name, created, restored, listed, renamed and deleted by commands, and restorable exactly from the GUI, reports and the journal. Restore reports any references that no longer resolve.

## Scope

**In:**
- NamedView entity: name (untrusted text), ViewState, digest, thumbnail reference
- save, restore, list, rename, delete commands
- Exact restore and the missing-reference report
- Headless restore check against the image at save time
- Serialisation of NamedViews for RPT and VIZ.WEB packages

**Out** (and which node owns it instead):
- ViewState model (`VIZ.VIEW.STATE`)
- Thumbnails as images (`VIZ.CAPTURE`)
- Report placement of views (`RPT`)

## Requirements

- **REQ-VIZ.VIEW.NAMED-001** `view.save_view@1` shall store the complete current ViewState with its digest under a name.
- **REQ-VIZ.VIEW.NAMED-002** When a NamedView is restored on the same document state, the restored ViewState's canonical bytes shall equal the saved bytes.
- **REQ-VIZ.VIEW.NAMED-003** When a NamedView is restored headlessly, the image shall match the image rendered at save time within the golden-image tolerance.
- **REQ-VIZ.VIEW.NAMED-004** If a NamedView references entities, colour maps or label sets that no longer exist, then restore shall apply the remaining state and return a list of every missing reference.
- **REQ-VIZ.VIEW.NAMED-005** Save, restore, rename and delete shall be registry commands recorded in the journal; list shall be R0.
- **REQ-VIZ.VIEW.NAMED-006** A deleted NamedView shall remain recoverable from journal history.

## Interfaces

- **Offers:** `view.save_view@1`, `view.restore_view@1`, `view.list_views@1` (R0), `view.rename_view@1`, `view.delete_view@1`; `NamedView` type consumed by RPT, VIZ.WEB and VIZ.CAPTURE.
- **Consumes:** `VIZ.VIEW.STATE`; `VIZ.RENDER.HEADLESS` for the restore check; CORE journal.

## Decomposition hints

SAVE, RESTORE (exact restore and missing references), LIST, RENAME, DELETE, EXPORT (serialisation for RPT and WEB).

## Open questions

- `view.rename_view` is not in the parent frontmatter; the parent's scope lists rename, so it is added here as a command name to confirm.

## Children

_None yet: status `open`._
