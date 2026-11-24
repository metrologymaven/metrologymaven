---
id: SCAN.CMP.DEV.PRIM
title: Signed deviation primitive and its batch form
level: 4
parent: SCAN.CMP.DEV
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
built_in: [fid-scan-cmp]
satisfies: [REQ-SCAN.CMP.DEV-001, REQ-SCAN.CMP.DEV-002, REQ-SCAN.CMP.DEV-005, REQ-SCAN.CMP.DEV-006]
covers: []
depends_on: [SCAN.CMP.DEV.FIELD, GEO.DIST.BATCH, GEO.DIST.SIGN, GEO.DIST.FACE.BODY, GEO.DIST.FACE.INCID, KRN.NUM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# SCAN.CMP.DEV.PRIM: Signed deviation primitive and its batch form

## Purpose

The single definition of "signed deviation" in SCAN: one actual point (with an optional normal) against a nominal surface, by method `normal` or `shortest`, with a maximum search distance, an angle cutoff and the material-outside sign rule. The batch form fills a `FieldBuilder` and is the only kernel path that may do so. SCAN.CMP.ACT, SCAN.SECT, SCAN.EDGE and SCAN.THK call it instead of redefining deviation.

## Scope

**In:**
- `NominalRef` with variants `Cad` (face set or body) and `Mesh`; further variants are added by consumers (`OrientedCloud` by `SCAN.CMP.ACT`, `PlanarCurve` by `SCAN.SECT.WHISK`)
- `deviation(point, normal?, nominal, params) -> DevResult`
- `deviation_batch(points, normals?, nominal, params, threads, cancel) -> FieldBuilder`
- Exact meaning of methods `normal` and `shortest` (resolves the parent's open question)

**Out** (and which node owns it instead):
- Closest-point projection and pseudo-normals (owned by `GEO.DIST`)
- Field storage and digest (owned by `SCAN.CMP.DEV.FIELD`)
- Streaming, alignment and selections (owned by `SCAN.CMP.DEV.SURFCAD`)

## Requirements

- **REQ-SCAN.CMP.DEV.PRIM-001** For method `shortest`, the primitive shall return the Euclidean distance from the point to its closest point f on the nominal within the search distance, signed positive when the point lies on the material-outside side as decided by GEO.DIST.SIGN (pseudo-normal at edges and vertices).
- **REQ-SCAN.CMP.DEV.PRIM-002** For method `normal`, the primitive shall return (a − f)·n_f, where f is the closest point and n_f the outward nominal normal at f, and shall return `NO_MATCH` when f lies on an open boundary of the nominal (face-set border or mesh free edge), so that points beyond the nominal's edge get no value.
- **REQ-SCAN.CMP.DEV.PRIM-003** If no nominal point lies within the maximum search distance s, then the primitive shall return `NO_MATCH` with no value.
- **REQ-SCAN.CMP.DEV.PRIM-004** If the actual point has a normal and the angle between it and n_f exceeds the cutoff θ (radians, in [0, π]), then the primitive shall return `NO_MATCH`; points without a normal shall skip this test.
- **REQ-SCAN.CMP.DEV.PRIM-005** The batch form shall write one result per input index into a `FieldBuilder`, compute every value on the CPU in f64, and give bit-identical results for any thread count and block size.
- **REQ-SCAN.CMP.DEV.PRIM-006** Edge and vertex foot points shall be tested exhaustively: on fixture `dev_cube_edges`, every point's value shall match an analytic oracle within 1e-12 m.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/cmp/dev_sphere` (CAD sphere r = 50 mm, points at radii 49.9 to 50.1 mm), then both methods return r − 50 mm within 1e-12 m with positive outside.
- **AC-2** Given fixture `fixtures/scan/cmp/dev_cube_edges` (points around edges and corners of a 20 mm cube), then `shortest` matches the analytic distance and sign within 1e-12 m everywhere, and `normal` matches inside face regions.
- **AC-3** Given fixture `fixtures/scan/cmp/dev_open_plate` (open 50 mm plate, points beyond its border), then `normal` returns `NO_MATCH` beyond the border and `shortest` returns the distance to the border.
- **AC-4** Given s = 1 mm and a point 2 mm away, the result is `NO_MATCH`; given θ = 30° and an actual normal at 45° to n_f, the result is `NO_MATCH`.
- **AC-5** Given 10⁶ points with 1, 3 and 16 threads and block sizes 64 and 4 096, the resulting field digests are identical.

## Design

- **API** (`fid-scan-cmp::dev`): `enum Method { Normal, Shortest }`; `DevParams { method, max_distance: f64, max_angle: f64 }`; `enum DevResult { Valid(f64), NoMatch(NoMatchReason), Excluded }`; `enum NominalRef<'a> { Cad(&'a dyn ProjectionTarget /* GEO.DIST.FACE.BODY */), Mesh(&'a MeshTarget) }`; `fn deviation(p, n: Option<[f64;3]>, nominal, params) -> DevResult`; `pub(crate) fn deviation_batch(..) -> Result<FieldBuilder, DevError>`.
- **Algorithm:** `project_batch` (GEO.DIST.BATCH) with `max_distance = s`; sign and normal at the foot from GEO.DIST.SIGN (mesh) or GEO.DIST.FACE.INCID (CAD); open-boundary test from the projection's entity kind (boundary edge or vertex); angle via clamped dot product and `fid-num` `acos`; dot products written without FMA.
- **Errors:** `scan.cmp.invalid_parameter` (s ≤ 0, θ outside [0, π]), `geo.frame.mismatch`, `scan.cmp.cancelled`.
- **Determinism:** D0.
- **Crate:** `fid-scan-cmp` (kernel tier, KNP-1).

## Test fixtures

Seeded synthetic `dev_sphere`, `dev_cube_edges`, `dev_open_plate` with analytic oracles (CC0).

## Build prompt

> In the Rust crate `fid-scan-cmp`, module `dev`, implement the deviation primitive. `deviation(p, normal?, nominal: NominalRef, params)`: project p onto the nominal within `max_distance` s (GEO.DIST batch `ProjectionTarget`; mesh targets or CAD face sets and bodies); none → `NoMatch(beyond_distance)`. Normal at the foot: face normal inside, GEO.DIST.SIGN pseudo-normal at mesh edges and vertices, GEO.DIST.FACE.INCID for CAD. If the actual normal is present and angle(n_a, n_f) > θ → `NoMatch(angle)`. Method `shortest`: signed Euclidean distance (sign from GEO.DIST.SIGN, positive material-outside). Method `normal`: (a − f)·n_f, but `NoMatch(off_edge)` when the foot lies on an open boundary of the nominal. `deviation_batch` processes fixed blocks in parallel writing by index into the crate-private `FieldBuilder` (module `field`); results must be bit-identical for any thread count or block size. Leave `NominalRef` open for added variants (`OrientedCloud`, `PlanarCurve`).
>
> Constraints: C2 (CPU only), CONSTITUTION §8, KNP-1 (no FMA, `fid-num` acos/sqrt, fixed-order reductions, no std HashMap/HashSet). Tests: AC-1..AC-5.

## Open questions

- The `normal` method definition (closest-point foot, off-edge points rejected) is chosen to match common incumbent behaviour [unverified]; record any difference users report against it.

## Children

_Leaf: none._
