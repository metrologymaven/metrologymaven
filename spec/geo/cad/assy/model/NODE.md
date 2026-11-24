---
id: GEO.CAD.ASSY.MODEL
title: Instances and per-instance entity IDs
level: 4
parent: GEO.CAD.ASSY
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
built_in: [fid-geo-cad]
built_pr: 581
satisfies: [REQ-GEO.CAD.ASSY-002, REQ-GEO.CAD.ASSY-005]
covers: []
depends_on: [GEO.CAD.TOPO.MODEL, GEO.FRAME.TREE]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GEO.CAD.ASSY.MODEL: Instances and per-instance entity IDs

## Purpose

The data model of a placed occurrence of a body: an `Instance` names its definition body, its frame and its assembly path, and exposes the definition's faces, edges and vertices under per-instance IDs, so two placements of one bolt are distinct and stable across re-import. Geometry is never copied: consumers read the definition and the instance frame.

## Scope

**In:**
- `Instance { id, definition: BodyId, frame: FrameId, path: InstancePath }`
- `InstancePath`: ordered occurrence IDs from the root assembly (imported labels or derived IDs), canonical byte form
- `InstanceEntityId`: UUIDv5 over the instance path bytes and the definition entity's `TopologyId` bytes
- Two-way mapping tables (instance entity ID ↔ (instance, definition entity)) stored sorted
- `InstanceView` that hands out definition geometry plus the instance `FrameId`

**Out** (and which node owns it instead):
- Building instances from an assembly tree (owned by `GEO.CAD.ASSY.FLAT`)
- Commands (owned by `GEO.CAD.ASSY.CMD`)
- Applying transforms (owned by `GEO.FRAME`)

## Requirements

- **REQ-GEO.CAD.ASSY.MODEL-001** Each instance entity ID shall be UUIDv5 over a fixed project namespace and `path bytes | 0x00 | definition TopologyId bytes`, so the same assembly re-imported yields the same IDs and two instances of one body yield disjoint ID sets.
- **REQ-GEO.CAD.ASSY.MODEL-002** The mapping from instance entity ID to (instance, definition entity) and back shall be answered in O(log n) from sorted vectors, with `UnknownEntity` for absent IDs.
- **REQ-GEO.CAD.ASSY.MODEL-003** `InstanceView` shall return definition geometry together with the instance `FrameId` and shall offer no method that returns transformed copies of geometry.
- **REQ-GEO.CAD.ASSY.MODEL-004** If an instance's frame is not present in the supplied frame tree, then constructing the view shall fail with `UnknownFrame`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/cad/assy/two_bolts.json` (one bolt body placed twice under one assembly), when instances are built, then each has 2 × (faces + edges + vertices) distinct IDs across the pair and the IDs equal the committed vectors.
- **AC-2** Given the same fixture re-built in reverse occurrence order of input, then the IDs per path are unchanged.
- **AC-3** Given 10 000 seeded lookups (seed 5), when mapped forward and back, then every round trip returns the original.
- **AC-4** (error path) Given an instance whose frame is missing from the tree, then `UnknownFrame`.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `Instance`, `InstancePath(Vec<TopologyId>)`, `InstanceEntityId([u8; 16])`, `InstanceIndex { forward: Vec<(InstanceEntityId, InstanceIx, EntityIx)>, backward: Vec<(InstanceIx, EntityIx, InstanceEntityId)> }`.
- **Algorithm:** path bytes are length-prefixed concatenations of `TopologyId` encodings; IDs computed once per instance; tables sorted. Pinned in the build: `TopologyId` bytes are the GEO.CAD.TOPO.MODEL encoding v1 `id` field (`0x00` + 16 UUID bytes, or `0x01` + u16 LE length + UTF-8 label); path bytes are a u32 LE component count followed by each component's `TopologyId` bytes; the namespace is UUIDv5 of `https://github.com/metrologymaven/metrologymaven/spec/geo/cad/assy/model#instance-entity-id-v1` in the RFC 9562 URL namespace (`fc8f8d57-d822-54d4-8c14-fecaeb696456`); `Instance.id` is UUIDv5(namespace, path bytes). Paths hold 1 to 256 components.
- **Errors:** `UnknownEntity`, `UnknownFrame`. The build also reports `EmptyPath`, `PathTooDeep`, `TooLarge`, `DuplicateBody`, `DuplicatePath`, `UnknownDefinition` (also for a view over the wrong body) and `DuplicateId` (a SHA-1 collision; fail-closed).
- **Determinism:** D0.
- **Performance budget:** ID derivation for 10⁶ instance entities under 1 s [unverified].
- **Crate:** `fid-geo-cad` (L0, KNP-1 applies).

## Test fixtures

- `fixtures/geo/cad/assy/two_bolts.json`: a hand-written assembly description over a small bolt body (cylinder and planes). CC-BY-4.0.
- `fixtures/geo/cad/assy/expected.json`: the committed instance and instance entity IDs, written with `two_bolts.json` by the independent stdlib reference `fixtures/geo/cad/assy/make_assy_fixtures.py` (which reuses the GEO.CAD.TOPO.MODEL reference for the definition IDs). CC-BY-4.0.

## Build prompt

> In crate `fid-geo-cad` (L0, no I/O, KNP-1), add module `assy::model` with `Instance { id, definition, frame: FrameId, path: InstancePath }`, `InstanceEntityId` = UUIDv5(fixed namespace, `path bytes | 0x00 | TopologyId bytes`), an `InstanceIndex` of sorted forward and backward tables with O(log n) lookup (`UnknownEntity`), and `InstanceView` returning definition geometry plus the `FrameId` (never transformed copies); constructing a view checks the frame exists in a `fid-geo-frame` `FrameTree` (`UnknownFrame`). Acceptance: AC-1 distinct, committed IDs for two bolts; AC-2 input-order independence; AC-3 10 000 round trips; AC-4 `UnknownFrame`. Constraints: CONSTITUTION §5 stable IDs, §8 (no panic, no `unsafe`), no hash containers. Files: `crates/fid-geo-cad/src/assy/model.rs`, `tests/assy_model.rs`, `fixtures/geo/cad/assy/two_bolts.json`.

## Open questions

- None.

## Children

_Leaf: none._
