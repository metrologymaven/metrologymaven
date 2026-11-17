---
id: SCAN.MESHOP.REPAIR.FIXTOPO
title: Non-manifold, duplicate, degenerate and orientation fixes
level: 4
parent: SCAN.MESHOP.REPAIR
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.MESHOP.REPAIR-005, REQ-SCAN.MESHOP.REPAIR-006]
covers: []
depends_on: [SCAN.MESHOP.REPAIR.DETECT, SCAN.MESHOP.PROV]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# SCAN.MESHOP.REPAIR.FIXTOPO: Non-manifold, duplicate, degenerate and orientation fixes

## Purpose

The topological fixes that need no geometry construction beyond collapsing: remove duplicate and degenerate triangles, remove fins on non-manifold edges, split non-manifold vertices into one copy per fan, and unify winding per connected component. Each fix is a pass over the `MeshOpBuilder`, so flags follow the PROV rules.

## Scope

**In:**
- Pass order: duplicates → degenerates → non-manifold edges → non-manifold vertices → orientation
- Non-manifold edge rule: keep the two triangles whose dihedral angle across the edge is closest to flat, ties lowest indexes; remove the others
- Orientation: breadth-first flood per component from its lowest-index triangle, flipping neighbours to agree; the component's final orientation chosen so its signed volume is positive when closed

**Out** (and which node owns it instead):
- Self-intersection removal (owned by `SCAN.MESHOP.REPAIR.FIXSELF`)
- The command and before/after report (owned by `SCAN.MESHOP.REPAIR.RCMD`)

## Requirements

- **REQ-SCAN.MESHOP.REPAIR.FIXTOPO-001** After the passes, the mesh shall have zero duplicate triangles, zero degenerate triangles, zero non-manifold edges, zero non-manifold vertices and zero winding inconsistencies as measured by `detect_defects`, unless a component is non-orientable, which shall be reported by component index.
- **REQ-SCAN.MESHOP.REPAIR.FIXTOPO-002** A non-manifold vertex shall be split into one vertex per edge-connected fan, the copies ordered by the fan's lowest triangle index, with the original index kept by the first fan.
- **REQ-SCAN.MESHOP.REPAIR.FIXTOPO-003** Triangles kept unchanged shall be copied with `copy_triangle`, flipped triangles shall be produced with `derive_triangle`, and no triangle shall be created from nothing.
- **REQ-SCAN.MESHOP.REPAIR.FIXTOPO-004** The passes shall be pure and determinism class D0 and shall return per-pass counts of removed, split and flipped elements.

## Acceptance criteria

- **AC-1** Given `fixtures/scan/mesh/defects_mixed` without its self-intersections (variant `defects_topo`), after FIXTOPO `detect_defects` reports zero for every topological kind.
- **AC-2** Given a bow-tie vertex (two fans), the output has two vertices at the same position and the fans are disconnected at it.
- **AC-3** Given a Möbius strip fixture, the orientation pass reports component 0 as non-orientable and leaves winding consistent everywhere except along one seam edge, which is listed.
- **AC-4** Given a flipped synthesized triangle, its flipped output is flagged synthesized.

## Design

- **API** (`fid-scan-mesh::repair::topo`): `fn fix_topology(mesh: &MeshView, opts: &TopoOptions, out: &mut MeshOpBuilder) -> Result<TopoStats, MeshError>`.
- **Algorithm:** sorted keys for duplicates; dihedral by `fid-num` `atan2` of cross and dot products; fan union-find; orientation BFS queue in ascending triangle index; signed volume by fixed-order summation.
- **Errors:** `scan.mesh.invalid_index`.
- **Determinism:** D0.
- **Crate:** `fid-scan-mesh`.

## Test fixtures

`defects_topo`, `bowtie`, `mobius` (generated, CC0).

## Build prompt

> In the Rust crate `fid-scan-mesh`, module `repair::topo`, implement `fix_topology(mesh, opts, out)` writing through `MeshOpBuilder`: remove duplicates and degenerates; on non-manifold edges keep the two triangles with the dihedral angle closest to flat (ties lowest indexes); split non-manifold vertices per fan (first fan keeps the index); unify winding per component by BFS from the lowest triangle, choosing positive signed volume for closed components, and report non-orientable components and their seam edges. Return per-pass counts. Constraints: KNP-1 (no FMA, fixed-order sums, no std HashMap/HashSet), §8. Tests: AC-1..AC-4.

## Open questions

- None.

## Children

_Leaf: none._
