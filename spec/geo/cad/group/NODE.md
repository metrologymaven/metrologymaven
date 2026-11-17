---
id: GEO.CAD.GROUP
title: CAD groups and layers
level: 3
parent: GEO.CAD
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-GEO.CAD-007]
covers: [GOM.CAD.02, SA.CAD.03]
depends_on: [GEO.TYPES.REG, CORE.REG, CORE.DEP]
side_effect: W2
determinism: D0
cli: [fid geo create-group, fid geo rename-group, fid geo delete-group, fid geo assign-group, fid geo create-layer, fid geo rename-layer, fid geo delete-layer, fid geo assign-layer, fid geo show-layer, fid geo hide-layer]
mcp: [geo_create_group, geo_rename_group, geo_delete_group, geo_assign_group, geo_create_layer, geo_rename_layer, geo_delete_layer, geo_assign_layer, geo_show_layer, geo_hide_layer]
journal: [geo.create_group@1, geo.rename_group@1, geo.delete_group@1, geo.assign_group@1, geo.create_layer@1, geo.rename_layer@1, geo.delete_layer@1, geo.assign_layer@1, geo.show_layer@1, geo.hide_layer@1]
updated: 2026-11-16
---

# GEO.CAD.GROUP: CAD groups and layers

## Purpose

Lets users organise CAD the way incumbents do: named groups of bodies and faces (for example "datum faces" or "sheet flange") and CAD layers with visibility, created, renamed, deleted and assigned through journaled commands. Groups and layers are document data that selections and reports refer to; they never change geometry.

## Scope

**In:**
- `CadGroup { id, name, members }` where a member is a body ID or a (body ID, face ID) pair; a member may be in many groups
- `CadLayer { id, name, members, visible }` where a body or face is in at most one layer; imported layer names from IO become layers
- Ten W2 commands: create, rename, delete and assign for groups and layers, plus show and hide for layers
- Refusing to delete a group or layer that a dependent (for example a semantic selection) references
- GOM.CAD.02 (groups half) and SA.CAD.03 (CAD layer management)

**Out:**
- Part and assembly handling (owned by `GEO.CAD.ASSY`)
- Selecting faces by group or layer (owned by `GEO.SEL.QUERY`)
- How hidden layers are drawn (owned by `VIZ`)
- Topology and face IDs (owned by `GEO.CAD.TOPO`)

## Requirements

- **REQ-GEO.CAD.GROUP-001** Each of the ten group and layer commands shall be class W2 and shall produce exactly one Record Journal entry when it commits.
- **REQ-GEO.CAD.GROUP-002** Group and layer names shall be unique within a document, case-sensitive, 1 to 128 Unicode scalar values, and treated as untrusted data (C6); a duplicate name shall fail with `name_taken`.
- **REQ-GEO.CAD.GROUP-003** When a body or face is assigned to a layer, the system shall remove it from its previous layer in the same command, so membership in at most one layer always holds.
- **REQ-GEO.CAD.GROUP-004** If an assigned member's body or face ID does not exist, then the command shall fail with `unknown_member` naming the first such ID and journal nothing.
- **REQ-GEO.CAD.GROUP-005** If a group or layer to be deleted has dependents in the CORE dependency graph, then deletion shall fail with `in_use` listing up to 20 dependent IDs.
- **REQ-GEO.CAD.GROUP-006** Members shall be stored in ascending ID order, so describe output and digests are deterministic.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/cad/bracket_groups` (a test project with one body of 24 faces and two imported layers), when `fid geo create-group --name datum_faces --json` and `fid geo assign-group datum_faces --add <f1> <f2> <f3> --json` run, then the group holds the three faces in ascending ID order and the Record Journal grows by exactly two entries.
- **AC-2** Given the project, when a face in layer `L1` is assigned to `L2`, then it is in `L2` only and one journal entry is written.
- **AC-3** (error path) Given a second `create-group --name datum_faces`, when run, then it fails with `name_taken` and the journal is unchanged.
- **AC-4** (error path) Given a semantic selection depending on group `datum_faces` (mocked dependency edge), when `delete-group` runs, then it fails with `in_use` listing the selection ID.
- **AC-5** (error path) Given `assign-group` with a non-existent face ID, then it fails with `unknown_member`.
- **AC-6** (replay) Given the journal from AC-1 and AC-2, when replayed in exact mode, then the outcome is `IDENTICAL`.
- **AC-7** Given the generated manifest, then the ten tools exist and are projected as named tools (W2; agents write to their branch).

## Design

- **Registry command(s):** `geo.create_group@1 { name }`, `geo.rename_group@1 { group, name }`, `geo.delete_group@1 { group }`, `geo.assign_group@1 { group, add: [Member], remove: [Member] }`, and the same four for layers, plus `geo.show_layer@1 { layer }` and `geo.hide_layer@1 { layer }`; all W2, determinism D0; results return the new state digest.
- **Data structures:** `Member { body: EntityId, face: Option<TopologyId> }`; `CadGroup`, `CadLayer` with `BTreeSet<Member>`; stored as CORE elements registered via GEO.TYPES.REG (IDs UUIDv7).
- **Algorithm:** validation, then a single element update inside the command transaction; layer reassignment removes the member from the old layer in the same transaction.
- **Errors:** `name_taken`, `bad_name`, `unknown_member`, `in_use`, `not_found`.
- **Determinism:** D0.
- **Performance budget:** assigning 10 000 faces under 50 ms [unverified].
- **Crate:** `fid-geo-cmd` (document-side GEO state; outside the kernel graph because it computes no values of record).

## Test fixtures

- `fixtures/geo/cad/bracket_groups`: a test project built by a script from a synthetic body description (24 face IDs, no real CAD geometry needed; topology lookups mocked through a `TopologyLookup` trait until GEO.CAD.TOPO is a leaf). CC-BY-4.0.

## Build prompt

> In crate `fid-geo-cmd`, implement CAD groups and layers as CORE document elements and register ten W2 commands (CLI and MCP generated from the manifest; agents' calls commit to their branch): `geo.create_group@1`, `geo.rename_group@1`, `geo.delete_group@1`, `geo.assign_group@1 {group, add, remove}`, `geo.create_layer@1`, `geo.rename_layer@1`, `geo.delete_layer@1`, `geo.assign_layer@1`, `geo.show_layer@1`, `geo.hide_layer@1`. A member is a body ID or (body ID, face ID); groups may overlap; a member is in at most one layer (reassignment removes it from the old layer in the same transaction). Names unique, 1–128 Unicode scalars, untrusted data. Validate member IDs through a `TopologyLookup` trait (`unknown_member`); refuse deletion when CORE.DEP lists dependents (`in_use`, up to 20 IDs). Store members in `BTreeSet` order. Acceptance: AC-1..AC-7 (create and assign with two journal entries; layer move; `name_taken`; `in_use`; `unknown_member`; exact replay `IDENTICAL`; ten named tools in the manifest). Constraints: CONSTITUTION §3 classes and naming, C6 for names, §8 (no panic, no `unsafe`), no std HashMap in digested state. Files: `crates/fid-geo-cmd/src/cad_groups.rs`, `tests/cad_groups.rs`, `fixtures/geo/cad/bracket_groups/`.

## Open questions

- Whether groups may nest (groups of groups), as some incumbents allow [unverified]; this leaf keeps them flat. Nesting would be an additive later change.

## Children

_Leaf: none._
