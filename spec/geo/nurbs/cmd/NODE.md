---
id: GEO.NURBS.CMD
title: Curve and surface evaluation and inversion commands
level: 3
parent: GEO.NURBS
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-GEO.NURBS-001, REQ-GEO.NURBS-005]
covers: []
depends_on: [GEO.NURBS.EVAL, GEO.NURBS.ANALYTIC, GEO.NURBS.INVERT, CORE.REG]
side_effect: R1
determinism: D0
cli: [fid geo evaluate-curve, fid geo evaluate-surface, fid geo invert-point]
mcp: [geo_evaluate_curve, geo_evaluate_surface, geo_invert_point]
journal: [geo.evaluate_curve@1, geo.evaluate_surface@1, geo.invert_point@1]
updated: 2026-11-16
---

# GEO.NURBS.CMD: Curve and surface evaluation and inversion commands

## Purpose

Registry commands that evaluate a curve or surface and invert points onto it, so scripts, tests and agents can probe exact geometry and cross-check results without Rust.

## Scope

**In:**
- `geo.evaluate_curve@1`, `geo.evaluate_surface@1` (R1): positions, derivatives, normal and curvature at given parameters
- `geo.invert_point@1` (R1): closest point on a curve or surface for given query points
- Geometry given inline as a JSON definition (NURBS or analytic), at most 10 000 parameters or points per call

**Out:**
- Referencing the surface of a CAD face by entity ID (added once `GEO.CAD.TOPO` is a leaf; see Open questions)
- Trimmed-face projection (owned by `GEO.DIST`)

## Requirements

- **REQ-GEO.NURBS.CMD-001** The three commands shall be class R1, MCP exposed, and write no Record Journal entry.
- **REQ-GEO.NURBS.CMD-002** When an inline definition is invalid, the command shall return the GEO.NURBS.DATA or ANALYTIC error as a typed error code with the offending index.
- **REQ-GEO.NURBS.CMD-003** Every returned f64 shall be given both as a decimal string with 17 significant digits and as IEEE hex, so results round-trip exactly.
- **REQ-GEO.NURBS.CMD-004** If more than 10 000 parameters or points are given, then the command shall fail with `too_many_queries`.

## Acceptance criteria

- **AC-1** Given the bicubic surface in `fixtures/geo/nurbs/ref_set_v1` as inline JSON, when `fid geo evaluate-surface --def s.json --params p.json --order 2 --json` runs, then values equal the library output bit-for-bit (via the hex fields).
- **AC-2** Given a sphere definition and 10 query points, when `fid geo invert-point --def sphere.json --points q.json --json` runs, then foot points match GEO.NURBS.ANALYTIC results and the Record Journal is unchanged.
- **AC-3** (error path) Given a definition with a negative weight at index 3, when evaluated, then the error is `bad_weight` with index 3.
- **AC-4** The manifest lists the three tools as exposed.

## Design

- **Registry command(s):** `geo.evaluate_curve@1` R1 `{ def: CurveDef, params: [t], order: 0..2 }`; `geo.evaluate_surface@1` R1 `{ def: SurfaceDef, params: [[u, v]], order: 0..2 }`; `geo.invert_point@1` R1 `{ def: CurveDef | SurfaceDef, points: [[x, y, z]] }`. Lengths in metres.
- **Data structures:** JSON schemas for `CurveDef` and `SurfaceDef` (tagged by type).
- **Algorithm:** thin adapters.
- **Errors:** `bad_knots`, `count_mismatch`, `bad_weight`, `bad_degree`, `outside_domain`, `non_convergence`, `too_many_queries`.
- **Determinism:** D0.
- **Performance budget:** adapter overhead under 1 ms [unverified].
- **Crate:** `fid-geo-cmd`.

## Test fixtures

- `ref_set_v1` and `analytic_set.json` from GEO.NURBS.REF.

## Build prompt

> In crate `fid-geo-cmd`, register three R1 commands (MCP exposed, Activity Log only; CLI and MCP generated from the manifest): `geo.evaluate_curve@1 {def, params, order}`, `geo.evaluate_surface@1 {def, params, order}`, `geo.invert_point@1 {def, points}`. `def` is an inline tagged JSON definition of a NURBS or analytic curve or surface, validated by `fid-geo-nurbs`; at most 10 000 parameters or points (`too_many_queries`). Every f64 output is given as a 17-significant-digit decimal and as IEEE hex. Map library errors to codes `bad_knots`, `count_mismatch`, `bad_weight`, `bad_degree`, `outside_domain`, `non_convergence` with indexes. Acceptance: AC-1 bit-exact evaluation via hex; AC-2 inversion matches the library with no journal growth; AC-3 `bad_weight` at index 3; AC-4 three exposed tools. Constraints: CONSTITUTION §3, §8 (no panic, no `unsafe`), C6 (definitions are data). Files: `crates/fid-geo-cmd/src/nurbs_cmds.rs`, `tests/nurbs_cmds.rs`.

## Open questions

- Referencing a CAD face's surface by entity ID should be added as a minor, additive parameter once GEO.CAD.TOPO defines face handles.

## Children

_Leaf: none._
