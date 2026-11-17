---
id: VIZ.RENDER.SCENE.IDTAB
title: Render ID allocation and entity lookup tables
level: 4
parent: VIZ.RENDER.SCENE
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-VIZ.RENDER.SCENE-001, REQ-VIZ.RENDER.SCENE-002, REQ-VIZ.RENDER.SCENE-005]
covers: []
depends_on: [GEO.TYPES.DIGEST, CORE.PRJ.TREE]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.RENDER.SCENE.IDTAB: Render ID allocation and entity lookup tables

## Purpose

Give every drawable a dense, session-local u32 render ID bound to an entity ID or a derived ID, refuse drawables without identity, and resolve render ID to entity and entity to render IDs in constant time. The ID buffer and every pick go through these tables, so a stale ID must never resolve to the wrong entity.

## Scope

**In:** `RenderId`, `DerivedId`, `Identity` types; allocation; two-way lookup tables; generation guard against stale ID-buffer reads; rejection of identity-less drawables.
**Out:** projection of document state into nodes (`VIZ.RENDER.SCENE.PROJ`); stale status of results (`VIZ.RENDER.SCENE.STALE`); writing IDs into GPU buffers (`VIZ.RENDER.FRAME.GRAPH`); resolving hits to f64 geometry (`VIZ.PICK.RESOLVE`).

## Requirements

- **REQ-VIZ.RENDER.SCENE.IDTAB-001** The table shall allocate render IDs as dense u32 values starting at 1, reserving 0 for "no drawable", and shall never reuse a render ID within a session.
- **REQ-VIZ.RENDER.SCENE.IDTAB-002** Each render ID shall be bound to exactly one identity: an entity ID, or a derived ID made of a source entity ID plus a view-operation code.
- **REQ-VIZ.RENDER.SCENE.IDTAB-003** If a drawable is registered with neither an entity ID nor a derived ID, then registration shall fail with `MissingIdentity` and no render ID shall be allocated.
- **REQ-VIZ.RENDER.SCENE.IDTAB-004** Lookups from render ID to identity and from entity ID to its render IDs shall run in O(1) expected time and shall return entries in ascending render-ID order.
- **REQ-VIZ.RENDER.SCENE.IDTAB-005** When a render ID is retired, the table shall keep it as a tombstone so that a late ID-buffer read resolves to `Retired`, never to another entity.
- **REQ-VIZ.RENDER.SCENE.IDTAB-006** If allocation would exceed 2^32 - 2 IDs in a session, then the table shall fail with `IdSpaceExhausted` rather than wrap.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/scene-ids` (500 entity drawables and 300 derived drawables), when all are registered, then render IDs are 1..800 in registration order and every reverse lookup returns the registered identity.
- **AC-2** (error path) Given a drawable with no identity, when registered, then `MissingIdentity` is returned and the next successful registration receives the ID that would have been next.
- **AC-3** Given render ID 17 retired, when looked up, then `Retired` is returned; given a new registration, then it does not receive 17.
- **AC-4** Given an entity with 3 derived drawables, when entity-to-render lookup runs, then exactly its own render ID and the 3 derived IDs are returned in ascending order.
- **AC-5** (property) Given 10 000 random register/retire sequences (seed 21), then no render ID is ever bound to two identities.

## Design

- **Registry command(s):** none; library.
- **Data structures:** `RenderId(NonZeroU32)`; `DerivedId { source: EntityId, op: ViewOp }` with `ViewOp` an enum (`Whisker`, `Label`, `Leader`, `Ghost`, `ClipCap`, `Proxy`); `Identity = Entity(EntityId) | Derived(DerivedId)`; `IdTable { by_render: Vec<Slot>, by_entity: BTreeMap<EntityId, SmallVec<RenderId>> }` where `Slot = Live(Identity) | Retired(Identity)`.
- **Algorithm:** monotonic counter; `Vec` indexed by render ID for O(1) forward lookup; `BTreeMap` (no std HashMap, for deterministic iteration) for reverse lookup, with O(log n) worst case documented and benchmarked under the O(1)-expected budget of 1 µs.
- **Errors:** `MissingIdentity`, `IdSpaceExhausted`, `UnknownRenderId`.
- **Determinism:** D0 (same registration order gives same IDs).
- **Performance budget:** lookup under 1 µs; register 10^6 drawables under 200 ms [unverified].
- **Crate:** `fid-render` (module `scene::idtab`).

## Test fixtures

`fixtures/viz/scene-ids`: generated in-repo (seed 21), project-authored, CC0.

## Build prompt

> In crate `fid-render`, add module `scene::idtab`. Define `RenderId(NonZeroU32)` (0 means "no drawable" in the ID buffer), `DerivedId { source: EntityId, op: ViewOp }`, `Identity` and `IdTable`. `register(identity: Option<Identity>) -> Result<RenderId, IdError>` allocates monotonically from 1, never reuses IDs, fails `MissingIdentity` without consuming an ID, and fails `IdSpaceExhausted` instead of wrapping. `retire(id)` keeps a tombstone so lookups return `Retired`. Provide `identity(RenderId)` and `render_ids(EntityId)` (ascending). Use `Vec` for forward lookup and `BTreeMap` for reverse lookup (no std HashMap). Tests: AC-1..AC-5 (fixture `fixtures/viz/scene-ids`, property test seed 21). Rules: CONSTITUTION §8 (no panic, no `unsafe`, deny unwrap/expect/panic/todo/unimplemented/unreachable); C2: IDs are display identities only, never values of record.

## Open questions

- Settles the parent's question on recycling: render IDs are never recycled within a session; tombstones guard stale ID-buffer reads.

## Children

_Leaf: none._
