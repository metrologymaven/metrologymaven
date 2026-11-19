---
id: VIZ.RENDER.SCENE.PROJ
title: View-model projection and dirty-set re-projection
level: 4
parent: VIZ.RENDER.SCENE
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-render]
satisfies: [REQ-VIZ.RENDER.SCENE-003, REQ-VIZ.RENDER.SCENE-006, REQ-VIZ.RENDER.SCENE-007]
covers: []
depends_on: [VIZ.RENDER.SCENE.IDTAB, CORE.DEP.PROP, CORE.PRJ.TREE, GEO.TYPES.DIGEST, GEO.FRAME.TREE]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.RENDER.SCENE.PROJ: View-model projection and dirty-set re-projection

## Purpose

Build the scene graph from document state and keep it current cheaply: each element projects to scene nodes carrying kind, frame, f64 origin and bounds, style token, kernel-supplied status and the digest of the data they show; when CORE publishes a dirty set, only the bound nodes and their derived nodes are re-projected. Derived nodes (whiskers, labels, leaders) are regenerated from their source and never persisted.

## Scope

**In:** `SceneNode` and `SceneGraph`; the projector trait each element kind registers; full projection; dirty-set re-projection; derived-node regeneration; change stream for FRAME, LOD, SHADE and GLYPH.
**Out:** ID allocation (`VIZ.RENDER.SCENE.IDTAB`); stale marking (`VIZ.RENDER.SCENE.STALE`); overlay content (`VIZ.OVERLAY`); the dependency graph and dirty-set computation (`CORE.DEP.PROP`).

## Requirements

- **REQ-VIZ.RENDER.SCENE.PROJ-001** Every scene node shall record render ID, identity, kind, frame reference, f64 origin in metres, f64 bounds, style token, kernel-supplied status and the SHA-256 digest of the data it displays.
- **REQ-VIZ.RENDER.SCENE.PROJ-002** When a dirty set of entity IDs is published, the projector shall re-project only the nodes bound to those IDs and the derived nodes whose source is in the set, and shall leave every other node byte-identical.
- **REQ-VIZ.RENDER.SCENE.PROJ-003** The projector shall emit one change batch per dirty set listing added, updated and retired render IDs in ascending order.
- **REQ-VIZ.RENDER.SCENE.PROJ-004** The scene graph shall hold derived nodes in memory only and shall regenerate them from their source when the source digest changes.
- **REQ-VIZ.RENDER.SCENE.PROJ-005** Projecting the same document state twice shall produce scene graphs with identical node lists and digests.
- **REQ-VIZ.RENDER.SCENE.PROJ-006** If an element kind has no registered projector, then projection shall skip it, record a `NoProjector` diagnostic naming the kind, and continue.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/scene-doc-small` (a part with 1 mesh, 2 point sets, 12 characteristics with labels), when projected, then the node count, kinds and digests equal the stored expectation file.
- **AC-2** Given the same document with one characteristic changed and its dirty set published, then the change batch updates exactly that characteristic's node and its label and leader nodes, and all other nodes are unchanged.
- **AC-3** Given two projections of the same state, then the serialised node lists are byte-identical.
- **AC-4** (error path) Given an element of an unregistered kind, then projection completes and reports one `NoProjector` diagnostic.
- **AC-5** Given a saved project reopened, then no derived node is read from storage and all are regenerated.

## Design

- **Registry command(s):** none; library.
- **Data structures:** `SceneNode { id: RenderId, identity: Identity, kind: NodeKind, frame: FrameId, origin_m: [f64;3], bounds_m: Aabb64, style: StyleToken, status: DisplayStatus, data_digest: Digest }`; `SceneGraph` (ordered by render ID); `trait Projector { fn kind(&self) -> ElementKind; fn project(&self, el: &ElementView, out: &mut NodeSink) -> Result<(), ProjError>; }`; `ChangeBatch { added, updated, retired }`.
- **Algorithm:** full projection walks the element tree in tree order; re-projection takes the dirty set (`CORE.DEP.PROP`), looks up render IDs via IDTAB, re-runs the projector per element, regenerates derived nodes whose source changed, diffs by data digest.
- **Errors:** `NoProjector` (diagnostic), `ProjError` from projectors (node marked `error` status, never dropped silently).
- **Determinism:** D0.
- **Performance budget:** re-project a 100-element dirty set in a 10^5-node scene under 5 ms [unverified].
- **Crate:** `fid-render` (module `scene::proj`).

## Test fixtures

`fixtures/viz/scene-doc-small` and its expectation file, project-authored, CC0.

## Build prompt

> In crate `fid-render`, add module `scene::proj`. Define `SceneNode` (render ID, identity, kind, frame, f64 origin and bounds in metres, style token, kernel-supplied display status, data digest), `SceneGraph` ordered by render ID, a `Projector` trait registered per element kind, and `ChangeBatch { added, updated, retired }` sorted ascending. Full projection walks the element tree in order; on a CORE dirty set re-project only bound nodes plus derived nodes whose source is dirty; derived nodes (whiskers, labels, leaders) live in memory only and are regenerated. Missing projector: skip with a `NoProjector` diagnostic. Use the ID tables from `scene::idtab`. Tests AC-1..AC-5 with `fixtures/viz/scene-doc-small`. Rules: CONSTITUTION §8; C2 (no value of record computed here; status comes from the kernel); no std HashMap.

## Open questions

- None.

## Children

_Leaf: none._
