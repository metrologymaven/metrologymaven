---
id: VIZ.VIEW
title: View state and saved views
level: 2
parent: VIZ
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ-005]
covers: [GOM.VIZ.01, GOM.VIZ.02, GOM.VIZ.03, GOM.VIZ.06, PW.VIZ.02]
depends_on: [VIZ.RENDER, VIZ.OVERLAY, CORE]
cli: [fid view set-camera, fid view fly-to, fid view highlight, fid view set-clip, fid view set-visibility, fid view set-transparency, fid view save-view, fid view restore-view, fid view list-views, fid view delete-view]
mcp: [view_set_camera, view_fly_to, view_highlight, view_set_clip, view_set_visibility, view_set_transparency, view_save_view, view_restore_view, view_list_views, view_delete_view]
journal: [view.save_view@*, view.restore_view@*, view.delete_view@*, view.set_clip@*, view.set_visibility@*, view.set_transparency@*]
updated: 2026-11-16
---

# VIZ.VIEW: View state and saved views

## Purpose

Owns the complete, serialisable state of what a viewport shows and the commands that change it: camera, standard and custom orientations, clipping and section planes, visibility layers and explorer filters, per-entity transparency, highlight and fly-to, multi-viewport camera sync, and **NamedViews** that bundle all of it (with the active colour map and legend) so reports and the journal can restore a view exactly. It is separate because view state is a document artefact with a strict restore contract that RPT and replay depend on.

## Scope

**In:**
- ViewState model: camera (f64 pose, projection), clip planes, visibility, transparency, active ColorMapSpec and legend reference, label set reference, viewport layout
- Camera commands: set, standard orientations, orbit/pan/zoom gestures, fly-to an entity, highlight (including agent-proposed ghost overlays labelled as proposals)
- Clipping planes, boxes and section planes (state; rasterization is in VIZ.RENDER)
- Visibility: layers, element-tree filters, explorer filters by type, status, tag or query
- Transparency per entity and per group
- NamedView: save, restore, list, rename, delete; content-addressed digest
- Multi-viewport layouts with synced or independent cameras, before/after split
- Serialisation of ViewState for reports (RPT) and the browser package (VIZ.WEB)

**Out** (and which node owns it instead):
- Colour-map and label content (`VIZ.OVERLAY`; a NamedView only references them)
- Drawing clip planes and transparency (`VIZ.RENDER`)
- Camera animations and the view track for recording (`VIZ.CAPTURE`)
- Dock and panel layout of the application window (`VIZ.SHELL`)
- Report page layout (`RPT`)

## Requirements

- **REQ-VIZ.VIEW-001** A NamedView shall capture camera, projection, clip planes, visibility, transparency, colour-map and legend references, and label-set reference.
- **REQ-VIZ.VIEW-002** When a NamedView is restored on the same document state, the system shall reproduce a ViewState whose canonical serialisation is byte-identical to the saved one.
- **REQ-VIZ.VIEW-003** When a NamedView is restored headlessly, the rendered image shall match the image rendered at save time within the VIZ.RENDER golden-image tolerance.
- **REQ-VIZ.VIEW-004** If a NamedView references an entity that no longer exists, then restore shall succeed for the remaining state and shall report every missing reference.
- **REQ-VIZ.VIEW-005** Saving, restoring, renaming and deleting NamedViews, and changes to clipping, visibility and transparency on the working document, shall be registry commands recorded in the journal.
- **REQ-VIZ.VIEW-006** Transient camera motion (orbit, pan, zoom) shall not create journal entries; it shall be captured only by the view track (VIZ.CAPTURE).
- **REQ-VIZ.VIEW-007** The system shall support at least 6 independent clip or section planes per viewport [unverified].
- **REQ-VIZ.VIEW-008** When an agent issues highlight or fly-to, the system shall label the resulting overlay as a proposal and shall not change document state.
- **REQ-VIZ.VIEW-009** Explorer filters shall be expressible as semantic queries so that the same filter selects the same entities from GUI, CLI and MCP.

## Interfaces

- **Offers:** the `view.*` state commands listed in the frontmatter; `ViewState` and `NamedView` types (consumed by RPT restorable views, VIZ.WEB packages and VIZ.CAPTURE).
- **Consumes:** VIZ.RENDER for drawing; VIZ.OVERLAY ColorMapSpec and LabelSet references; CORE journal and element tree.

## Decomposition hints

STATE (ViewState model and canonical form), CAMERA (camera commands, orientations, fly-to, highlight), CLIP, VISIB (visibility and explorer filters), TRANSP, NAMED (NamedView lifecycle), MULTI (multi-viewport and sync).

## Open questions

- Whether camera changes made inside a NamedView edit session are W2 (working document) or presentation-only is to be fixed with CORE at leaf level.

## Children

_None yet: status `open`._
