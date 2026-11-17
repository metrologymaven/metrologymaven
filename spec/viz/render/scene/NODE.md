---
id: VIZ.RENDER.SCENE
title: Semantic scene graph and ID tables
level: 3
parent: VIZ.RENDER
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.RENDER-005, REQ-VIZ.RENDER-007]
covers: []
depends_on: [GEO, CORE]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.RENDER.SCENE: Semantic scene graph and ID tables

## Purpose

The display-side projection of the document: scene nodes bound to entity IDs (QIF persistent IDs or derived IDs), the session-local render-ID tables that the ID buffer and picking use, re-projection driven by CORE dirty sets, and stale-state marking. Every other RENDER child draws scene nodes, so their identity and staleness rules live in one place.

## Scope

**In:**
- SceneNode model: render ID, entity ID or derived ID, kind, source digest, frame reference, f64 origin and bounds, style token, kernel-supplied status, visibility slot
- Render ID allocation and the render-ID to entity-ID table (both directions)
- View-model projection from document state; re-projection of dirty sets only
- Stale marking when upstream inputs change, cleared by a new source digest
- Derived nodes (whiskers, labels, leader lines) regenerated from their source, never persisted here

**Out** (and which node owns it instead):
- GPU buffers, passes and backends (`VIZ.RENDER.FRAME`)
- Chunk streaming of large clouds (`VIZ.RENDER.LOD`)
- Content of overlays and labels (`VIZ.OVERLAY`)
- Dependency graph and recompute (`CORE.DEP`)
- Resolving a render ID hit to f64 geometry (`VIZ.PICK`)

## Requirements

- **REQ-VIZ.RENDER.SCENE-001** Every scene node shall carry a session-local dense u32 render ID and either an entity ID or a derived ID made of a source entity ID plus a view operation.
- **REQ-VIZ.RENDER.SCENE-002** If a drawable with neither an entity ID nor a derived ID is submitted, then scene build shall reject it with a typed error and shall draw nothing for it.
- **REQ-VIZ.RENDER.SCENE-003** When CORE publishes a dirty set of entity IDs, the scene graph shall re-project only the nodes bound to those IDs and their derived nodes.
- **REQ-VIZ.RENDER.SCENE-004** When an upstream input of a displayed result changes, the scene graph shall set that node's status to stale until recompute delivers a new source digest, and a stale node shall never be styled as conforming or nonconforming.
- **REQ-VIZ.RENDER.SCENE-005** The scene graph shall resolve render ID to entity ID and entity ID to render IDs in constant time and shall expose both lookups to VIZ.PICK.
- **REQ-VIZ.RENDER.SCENE-006** Each scene node shall record the digest of the data it displays.
- **REQ-VIZ.RENDER.SCENE-007** The scene graph shall not persist derived nodes; it shall regenerate them from their source when the source changes.

## Interfaces

- **Offers:** `SceneNode`, `SceneGraph`, `RenderId`, `DerivedId` types; render-ID lookup tables; a change stream of re-projected nodes for FRAME and LOD.
- **Consumes:** CORE dirty sets and entity statuses (`CORE.DEP`); GEO geometry handles and frames (`GEO.TYPES`, `GEO.FRAME`).

## Decomposition hints

IDTAB (render ID allocation and tables), PROJ (view-model projection), DIRTY (dirty-set re-projection), STALE (stale marking and digest check).

## Open questions

- Whether render IDs are recycled within a session (and how stale ID-buffer reads are guarded) is to be fixed at leaf level with VIZ.PICK.
- Resolved in IDTAB: render IDs are never recycled within a session; tombstones guard stale ID-buffer reads (agreed boundary with `VIZ.PICK.RESOLVE`).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| VIZ.RENDER.SCENE.IDTAB | Render ID allocation and entity lookup tables (leaf) | 001 002 005 | none |
| VIZ.RENDER.SCENE.PROJ | View-model projection and dirty-set re-projection (leaf) | 003 006 007 | none |
| VIZ.RENDER.SCENE.STALE | Stale marking and digest check (leaf) | 004 | none |
