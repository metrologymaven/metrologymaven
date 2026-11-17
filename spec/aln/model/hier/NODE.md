---
id: ALN.MODEL.HIER
title: Alignment hierarchy, groups, frames and deletion
level: 3
parent: ALN.MODEL
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-ALN.MODEL-003, REQ-ALN.MODEL-004, REQ-ALN.MODEL-006]
covers: [GOM.ALN.08, PW.ALN.08, SA.XFM.02]
depends_on: [ALN.MODEL.ELEM, GEO.FRAME, CORE.DEP]
cli: [fid align set-parent, fid align create-frame, fid align create-group, fid align edit-group, fid align delete-alignment]
mcp: [align_set_parent, align_create_frame, align_create_group, align_edit_group, align_delete_alignment]
journal: [align.set_parent@1, align.create_frame@1, align.create_group@1, align.edit_group@1, align.delete_alignment@1]
updated: 2026-11-16
---

# ALN.MODEL.HIER: Alignment hierarchy, groups, frames and deletion

## Purpose

Chains alignments into a hierarchy (each alignment is relative to a parent), composes the chain to the root frame, groups several data elements under one transform, lets users create and nest named frames (including a frame built from an existing alignment), and governs deletion and invalidation when a parent changes or disappears. It is the alignment-side view of GEO's frame tree.

## Scope

**In:**
- Parent assignment and re-parenting of alignments, with cycle rejection
- Composition of an alignment's transform to the root frame through its ancestors
- Alignment groups: one transform applied to several data elements together (membership edits)
- Named frames: create, nest, and build from an existing alignment (SA-style frames)
- Deletion of alignments and frames, with dependent handling
- Invalidation of descendants when a parent alignment changes (through CORE.DEP)

**Out** (and which node owns it instead):
- The element type and digest (`ALN.MODEL.ELEM`)
- Which alignment is active (`ALN.MODEL.ACTIVE`)
- Transform types, composition math and the world-rooted frame tree (`GEO.FRAME`)
- Multi-station network frames (`LVM`)
- 3-2-1 frame construction from geometry (`ALN.RPS.CLASSIC`)

## Requirements

- **REQ-ALN.MODEL.HIER-001** When the transform from an alignment to the root frame is requested, the system shall return the ordered product of its ancestors' transforms, composed through GEO.FRAME in a fixed documented order, with a round trip within 1e-12 m for points within 10 m of the origin [unverified].
- **REQ-ALN.MODEL.HIER-002** If `align.set_parent` or any creating command would make an alignment its own ancestor, then the system shall reject it with `hierarchy_cycle` naming the cycle and journal nothing.
- **REQ-ALN.MODEL.HIER-003** When a data element is added to an alignment group, the group's single transform shall apply to it, and an element shall belong to at most one group at a time.
- **REQ-ALN.MODEL.HIER-004** The system shall create named frames, nest a frame under another frame or alignment, and build a frame whose transform equals a given alignment's composed transform at creation time, recording the source alignment's digest.
- **REQ-ALN.MODEL.HIER-005** When a parent alignment changes, the system shall mark every descendant alignment and dependent result `outdated` through CORE.DEP before any recompute.
- **REQ-ALN.MODEL.HIER-006** If an alignment to be deleted has child alignments or dependent results, then the system shall reject the deletion with `has_dependents` listing them, unless the user selects `re_parent` (children move to the deleted alignment's parent with composed transforms kept) or `cascade` (dependents are deleted too), each journaled.
- **REQ-ALN.MODEL.HIER-007** Hierarchy depth shall be limited to 64 levels, and deeper chains shall be rejected with `hierarchy_too_deep` [unverified: limit chosen for bounded composition].

## Interfaces

- **Offers:** `align.set_parent@1`, `align.create_frame@1`, `align.create_group@1`, `align.edit_group@1`, `align.delete_alignment@1` (all W2); `compose_to_root(alignment_id) -> Transform` used by `ALN.MODEL.ACTIVE`, `ALN.QA.CMP` and `ALN.XFM.LEAP`.
- **Consumes:** `ALN.MODEL.ELEM` element; GEO.FRAME transform composition and frame tree; CORE.DEP edges and invalidation.

## Decomposition hints

CHAIN (parent assignment, composition, cycles, depth), GROUP (alignment groups), FRAME (named frames and frame from alignment), DEL (deletion modes). CHAIN and DEL are likely S leaves; FRAME may need GEO.FRAME's leaf first.

## Open questions

- Whether a frame built from an alignment should stay live (follow the alignment) or be a snapshot. Current requirement: snapshot with source digest; a live frame is just a child alignment.

## Children

_None yet: status `open`._
