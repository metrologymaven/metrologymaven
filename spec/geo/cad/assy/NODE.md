---
id: GEO.CAD.ASSY
title: CAD parts, assemblies and instance placement
level: 3
parent: GEO.CAD
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-GEO.CAD-001]
covers: [GOM.CAD.02]
depends_on: [GEO.CAD.TOPO, GEO.FRAME.TREE, GEO.FRAME.CMD]
cli: [fid geo assign-part, fid geo place-instance]
mcp: [geo_assign_part, geo_place_instance]
journal: [geo.assign_part@1, geo.place_instance@1]
updated: 2026-11-16
---

# GEO.CAD.ASSY: CAD parts, assemblies and instance placement

## Purpose

Handles CAD that arrives as assemblies: a body definition may be placed several times, each placement is an instance with its own frame and its own stable sub-entity IDs, and instances are assigned to the parts the project inspects. The assembly is flattened into placed instances; hierarchy is kept only as provenance.

## Scope

**In:**
- Instance model: definition body plus placement (a GEO.FRAME frame whose parent is the assembly or part frame)
- Flattening of an imported assembly tree into instances, keeping the path as provenance
- Per-instance stable IDs for faces, edges and vertices: UUIDv5 over the instance path and the definition entity ID
- Part handling: assigning instances to project parts (CORE Part → Revision), removing them from a part, listing parts' CAD
- Commands `geo.assign_part@1` and `geo.place_instance@1` (W2)
- GOM.CAD.02 (part handling half; groups are in GEO.CAD.GROUP)

**Out** (and which node owns it instead):
- Reading assembly structure from files (owned by `IO.CAD`)
- Groups and layers (owned by `GEO.CAD.GROUP`)
- Frame tree and transforms (owned by `GEO.FRAME`)
- The Part → Revision container (owned by `CORE.PRJ`)

## Requirements

- **REQ-GEO.CAD.ASSY-001** When an assembly is imported, the system shall create one instance per placed occurrence, each with a frame whose transform equals the composed placement along its assembly path.
- **REQ-GEO.CAD.ASSY-002** Each instance shall expose its faces, edges and vertices under IDs derived by UUIDv5 from the instance path and the definition entity ID, so two instances of one body have distinct, re-import-stable IDs.
- **REQ-GEO.CAD.ASSY-003** If an assembly placement is not a rigid or uniform-similarity transform, then import shall report the instance as `unsupported_placement` and not place it.
- **REQ-GEO.CAD.ASSY-004** Assigning instances to a part and placing an instance shall each be a W2 command producing one journal entry.
- **REQ-GEO.CAD.ASSY-005** Every downstream operation on an instance (projection, selection, tessellation) shall see its geometry in the instance frame through GEO.FRAME, never by copying transformed geometry.

## Interfaces

- **Offers:** `Instance { id, definition: BodyId, frame: FrameId, path }`, `InstanceEntityId`, commands `geo.assign_part@1`, `geo.place_instance@1`.
- **Consumes:** GEO.CAD.TOPO bodies; GEO.FRAME tree and frame commands; CORE.PRJ parts.

## Decomposition hints

MODEL (instances and IDs), FLATTEN (assembly tree to instances), PART (part assignment commands).

## Open questions

- Whether non-uniform scale placements in real assemblies (mirrored parts, scaled imports) need support; mirrored instances need an improper transform, which GEO.FRAME rejects today.

## Children

_None yet: status `open`._
