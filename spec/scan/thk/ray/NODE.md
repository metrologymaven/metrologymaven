---
id: SCAN.THK.RAY
title: Ray-cast wall thickness
level: 3
parent: SCAN.THK
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.THK-001, REQ-SCAN.THK-002, REQ-SCAN.THK-005, REQ-SCAN.THK-006, REQ-SCAN.THK-007]
covers: [GOM.SRF.07, PW.CMP.05]
depends_on: [SCAN.THK.TFIELD, GEO.INDEX]
side_effect: W2
determinism: D0
cli: [fid compare thickness-wall]
mcp: [compare_thickness_wall]
journal: [compare.thickness_wall@1]
updated: 2026-11-16
---

# SCAN.THK.RAY: Ray-cast wall thickness

## Purpose

Wall thickness of a part scanned on both sides: from each point, cast a ray along the inward normal and measure the distance to the first opposite surface it hits. This node owns the `compare.thickness_wall@1` command, with `ray` as its default method; `SCAN.THK.SPHERE` adds the `sphere` method to the same command.

## Scope

**In:**
- `compare.thickness_wall@1` with `method` parameter (`ray` implemented here; `sphere` dispatched to `SCAN.THK.SPHERE`)
- Ray from each mesh vertex (or oriented cloud point) along the inward normal to the first opposite-surface hit
- Maximum thickness in metres and maximum angle between the ray and the hit surface normal
- Self-hit exclusion near the start point
- Cloud input by a ray corridor of stated radius
- Optional restriction to a selection

**Out** (and which node owns it instead):
- Inscribed-sphere method (owned by `SCAN.THK.SPHERE`)
- Material thickness to a nominal (owned by `SCAN.THK.MAT`)
- Field element, limits and statistics (owned by `SCAN.THK.TFIELD`)

## Requirements

- **REQ-SCAN.THK.RAY-001** When `compare.thickness_wall@1` runs with method `ray`, the system shall create a `ThicknessField` through `ThicknessFieldBuilder` recording the method, parameters and source digest, and shall leave the source bit-identical.
- **REQ-SCAN.THK.RAY-002** For each start point p with outward unit normal n, the thickness shall be the distance t from p to the first intersection of the ray p − s·n (s > ε) with the surface, where ε (default 1e-6 m) excludes self-hits on triangles incident to p.
- **REQ-SCAN.THK.RAY-003** A hit shall be accepted only if t ≤ the stated maximum thickness and the angle between the ray direction −n and the hit surface's outward normal is at most the stated cutoff; otherwise the point shall be `NO_MATCH` with no value.
- **REQ-SCAN.THK.RAY-004** For an oriented cloud, the hit shall be the first point along the ray within a corridor of stated radius r in metres around the ray, measured by its projection on the ray, ties by lowest point index.
- **REQ-SCAN.THK.RAY-005** Mesh vertices used only by synthesized triangles shall be `EXCLUDED`, and synthesized triangles shall not be accepted as hits.
- **REQ-SCAN.THK.RAY-006** The method shall be determinism class D0, and its documented thickness definition (definition ID `thk.ray.v1`) shall be recorded with the field.
- **REQ-SCAN.THK.RAY-007** On the stepped-plate fixture the method shall report the known thicknesses within 1 µm at points at least 2 mm from a step [unverified tolerance].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/thk_step_plate` (closed mesh of a plate with 2.0, 3.0 and 4.0 mm regions, seed 59), when `fid compare thickness-wall --method ray --max-thickness 0.01 --max-angle 0.35 --json` runs on the top surface, then points ≥ 2 mm from a step report 2.000, 3.000 and 4.000 mm within 1 µm.
- **AC-2** Given the same with `--max-thickness 0.0035`, the 4 mm region is `NO_MATCH` and has no values.
- **AC-3** Given fixture `fixtures/scan/thk_tube_cloud` (oriented cloud of a tube with 1.5 mm wall, both sides, seed 61) and corridor radius 0.2 mm, the median thickness is 1.5 mm ± 20 µm [unverified].
- **AC-4** Given a mesh with a filled (synthesized) patch on the bottom face, rays hitting it are `NO_MATCH` rather than measured.
- **AC-5** (error path) Given a cloud without normals, the command fails with `scan.thk.no_normals` and journals nothing.
- **AC-6** (replay) Exact replay of AC-1 on another thread count yields `IDENTICAL`.

## Design

- **Registry command:** `compare.thickness_wall@1`, W2, D0. Parameters: `source` (mesh or oriented cloud), `method` (`ray` default | `sphere`), `max_thickness` m, `max_angle` rad (default 0.35), `epsilon` m (default 1e-6), `corridor` m (clouds), `selection?`. Dispatch for `sphere` to `fid_scan_thk::sphere`.
- **Algorithm (mesh):** BVH ray query (GEO.INDEX) returning hits sorted by (s, triangle ID); skip hits with s ≤ ε or on triangles incident to the start vertex; Möller–Trumbore intersection evaluated without FMA.
- **Algorithm (cloud):** candidate points from a capsule query along the ray, first by projection then index.
- **Errors:** `scan.thk.no_normals`, `scan.thk.bad_parameter`, `geo.selection.stale`.
- **Determinism:** D0.
- **Performance budget:** 10⁷ vertices against a 2·10⁷-triangle mesh within 60 s [unverified].
- **Crate:** `fid-scan-thk`.

## Test fixtures

- `fixtures/scan/thk_step_plate` (seed 59, shared) and `fixtures/scan/thk_tube_cloud` (seed 61): generated, CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-scan-thk`, implement the W2, D0 registry command `compare.thickness_wall@1` (CLI `fid compare thickness-wall --source <h> [--method ray|sphere] --max-thickness M [--max-angle RAD] [--epsilon M] [--corridor M] [--selection <h>] [--json]`, MCP `compare_thickness_wall`). Method `ray` (default): from each start point p with outward normal n, cast p − s·n; thickness = first hit distance s > ε (default 1e-6 m), skipping triangles incident to p and synthesized triangles; accept only if s ≤ max thickness and the angle between −n and the hit triangle's outward normal ≤ max angle, else NO_MATCH. Oriented clouds: first point along the ray within a corridor radius (projection order, ties lowest index). Vertices used only by synthesized triangles are EXCLUDED. Method `sphere` dispatches to the `sphere` module (implemented separately). Write results through `ThicknessFieldBuilder` with definition ID `thk.ray.v1`.
>
> Constraints: CPU only, KNP-1 (no FMA, total-order hit sorting, no std HashMap/HashSet), use the GEO.INDEX BVH ray query, no panic/unwrap/unsafe, source never modified. Tests: stepped plate (2/3/4 mm within 1 µm away from steps), max-thickness NO_MATCH, tube cloud median, synthesized-hit rejection, missing normals error, exact replay IDENTICAL.

## Open questions

- None.

## Children

_Leaf: none._
