---
id: SCAN.MESHOP.REPAIR.RCMD
title: Mesh repair command and before/after report
level: 4
parent: SCAN.MESHOP.REPAIR
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-SCAN.MESHOP.REPAIR-001, REQ-SCAN.MESHOP.REPAIR-003, REQ-SCAN.MESHOP.REPAIR-005, REQ-SCAN.MESHOP.REPAIR-006, REQ-SCAN.MESHOP.REPAIR-007]
covers: [GOM.MSH.13]
depends_on: [SCAN.MESHOP.REPAIR.DETECT, SCAN.MESHOP.REPAIR.FIXTOPO, SCAN.MESHOP.REPAIR.FIXSELF, SCAN.MESHOP.PROV]
side_effect: W2
determinism: D0
cli: [fid mesh repair]
mcp: [mesh_repair]
journal: [mesh.repair@1]
updated: 2026-11-17
---

# SCAN.MESHOP.REPAIR.RCMD: Mesh repair command and before/after report

## Purpose

The user-facing `mesh.repair@1`: detect, fix topology, remove self-intersections, fix topology again, and report every defect count before and after, so the user sees exactly what changed.

## Scope

**In:**
- `mesh.repair@1` with switches for each fix family and the self-intersection ring count
- Pipeline: detect → FIXTOPO → FIXSELF → FIXTOPO → detect
- Before/after `DefectReport` counts in the result
- Performance budget at 10⁸ triangles

**Out** (and which node owns it instead):
- Detection and fixes (owned by DETECT, FIXTOPO, FIXSELF)
- Hole filling (owned by `SCAN.MESHOP.HOLES`)

## Requirements

- **REQ-SCAN.MESHOP.REPAIR.RCMD-001** When `mesh.repair@1` runs, the system shall create a new mesh version through `MeshOp` and leave the input bit-identical.
- **REQ-SCAN.MESHOP.REPAIR.RCMD-002** The result shall report, for every defect kind of `DefectReport`, the count before and after the repair, and the per-pass statistics.
- **REQ-SCAN.MESHOP.REPAIR.RCMD-003** If FIXSELF returns unresolved intersections, then the command shall still create the mesh only when `allow_partial` is true, and shall otherwise fail with `scan.mesh.unresolved_intersections` and create nothing.
- **REQ-SCAN.MESHOP.REPAIR.RCMD-004** Repair of a 10⁸-triangle mesh with fewer than 0.1 % defective triangles shall complete within 600 s and 32 GiB on reference hardware [unverified].
- **REQ-SCAN.MESHOP.REPAIR.RCMD-005** The command shall be determinism class D0.

## Acceptance criteria

- **AC-1** Given `fixtures/scan/mesh/defects_mixed`, when `fid mesh repair --mesh <m> --json` runs, then after-counts are zero for all kinds, before-counts match the plant list, and the input is unchanged.
- **AC-2** Given `self_knot` without `--allow-partial`, the command fails `scan.mesh.unresolved_intersections` and journals nothing; with it, a mesh is created and the after-count lists the remaining pairs.
- **AC-3** (replay) Replay of AC-1 in exact mode is `IDENTICAL`.
- **AC-4** (bench, advisory) REQ-004 is measured and published.

## Design

- **Registry command:** `mesh.repair@1` (W2, D0): `mesh`, `fix_topology` (default true), `fix_self_intersections` (default true), `rings` (default 1), `allow_partial` (default false), `degenerate_area?`.
- **Errors:** `scan.mesh.unresolved_intersections`, DETECT/FIXTOPO errors passed through.
- **Determinism:** D0.
- **Crate:** `fid-scan-mesh`.

## Test fixtures

`defects_mixed`, `self_knot` (shared), `bench_repair_1e8` (generated at CI time), CC0.

## Build prompt

> In the Rust crate `fid-scan-mesh`, module `repair::cmd`, register `mesh.repair@1` (W2, D0; CLI `fid mesh repair --mesh <h> [--no-topology] [--no-self] [--rings N] [--allow-partial] [--json]`, MCP `mesh_repair`). Pipeline through one `MeshOpBuilder`: `detect_defects` → `fix_topology` → `fix_self_intersections` → `fix_topology` → `detect_defects`; return before/after counts for every kind and per-pass statistics. Unresolved intersections fail `scan.mesh.unresolved_intersections` unless `allow_partial`. Constraints: §3, §8, KNP-1. Tests: AC-1..AC-4.

## Open questions

- None.

## Children

_Leaf: none._
