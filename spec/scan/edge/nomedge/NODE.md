---
id: SCAN.EDGE.NOMEDGE
title: Nominal edge chains and station frames
level: 3
parent: SCAN.EDGE
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
built_in: [fid-scan-edge]
built_pr: 691
satisfies: [REQ-SCAN.EDGE-001, REQ-SCAN.EDGE-002]
covers: []
depends_on: [KRN.CONS, GEO.CAD, GEO.NURBS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# SCAN.EDGE.NOMEDGE: Nominal edge chains and station frames

## Purpose

Turns a nominal edge (a chain of CAD boundary edges, or an edge or trim curve built by KRN.CONS) into an ordered list of stations at a stated spacing, each with the local frame needed to split a deviation into its trim and normal parts: the edge tangent, the nominal surface normal, and the in-surface direction perpendicular to the edge pointing out of the material. Both extraction and deviation use these stations.

## Scope

**In:**
- Nominal edge input: ordered CAD edges on one face, or a KRN.CONS edge or trim curve with its adjacent face
- Chaining and orientation of edges into one curve with arc-length parameterisation
- Stations at a stated arc-length spacing with frame (t, n, b): tangent, nominal surface normal, outward in-surface binormal
- Validation of the chain (connected, single face side, no zero-length gaps)

**Out** (and which node owns it instead):
- Creating edge or trim curves as constructions (owned by `KRN.CONS`)
- Exact curve and surface evaluation (owned by `GEO.NURBS` and `GEO.CAD`)
- Actual edge extraction (owned by `SCAN.EDGE.MESHBND` and `SCAN.EDGE.EST`)
- Deviation values and the command (owned by `SCAN.EDGE.EDEV`)

## Requirements

- **REQ-SCAN.EDGE.NOMEDGE-001** The station function shall chain the given CAD edges or curve into one oriented curve, and if the edges are not connected end to end within 1e-9 m or lie on more than one face side, then it shall fail with a typed error naming the gap.
- **REQ-SCAN.EDGE.NOMEDGE-002** The function shall place stations at arc-length spacing d in metres from the chain start, the last station at or before the end, and on a closed chain omit a station at the full length.
- **REQ-SCAN.EDGE.NOMEDGE-003** Each station shall carry a right-handed orthonormal frame (t, n, b) where t is the unit edge tangent, n is the unit outward normal of the adjacent nominal face at the station, and b = t × n is oriented to point away from the face material, with orthonormality within 1e-12.
- **REQ-SCAN.EDGE.NOMEDGE-004** The function shall be pure, use exact curve and surface evaluation (not tessellation), and be determinism class D0.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/edge_plate` (a 200 × 100 mm planar plate face in z = 0, outward normal +z, with its four boundary edges), when stations are generated with spacing 0.01 m on the +x edge, then there are 11 stations at y = −0.05 … +0.05 with t = ±y, n = +z and b = +x.
- **AC-2** Given fixture `fixtures/scan/edge_cylinder_trim` (a trim curve on a cylindrical face), every station's n equals the exact cylinder normal within 1e-12 and frames are orthonormal within 1e-12.
- **AC-3** (error path) Given two edges with a 1 mm gap, the function fails with `scan.edge.chain_gap` reporting the gap location.
- **AC-4** Given the closed four-edge loop of the plate with spacing 0.1 m, there are ceil(0.6/0.1) = 6 stations and no duplicate at the start.

## Design

- **API** (`fid-scan-edge::nominal`): `fn stations(edge: &NominalEdge, spacing: f64, resolver: &dyn CadResolver) -> Result<Vec<Station>, EdgeError>`; `Station { s_arc, point, t, n, b }`; `NominalEdge::{CadEdges(face, [edge ids]), Constructed(curve ref, face)}`.
- **Algorithm:** order edges by matching endpoints (lowest edge ID first); arc length by adaptive Gauss–Legendre quadrature with a fixed node set from `fid-num`; station parameter by bisection on arc length (fixed 60 iterations); n from the face's exact normal at the closest surface parameter (GEO.NURBS inversion).
- **Errors:** `scan.edge.chain_gap`, `scan.edge.multi_face`, `scan.edge.bad_spacing`.
- **Determinism:** D0.
- **Crate:** `fid-scan-edge` (kernel tier, KNP-1).

## Test fixtures

- `fixtures/scan/edge_plate`, `fixtures/scan/edge_cylinder_trim`: generated STEP models, CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-scan-edge`, module `nominal`, implement the pure D0 function `stations(edge, spacing, resolver)` that chains CAD boundary edges on one face (or takes a constructed edge or trim curve plus its face) into one oriented curve, fails with `scan.edge.chain_gap` / `scan.edge.multi_face` / `scan.edge.bad_spacing` on invalid input, and places stations every `spacing` metres of arc length (closed chains: no duplicate station at full length). Each station has a right-handed orthonormal frame: t = unit tangent, n = exact outward face normal at the station (via GEO.NURBS/GEO.CAD evaluation, not tessellation), b = t × n pointing away from the material. Use fixed-node quadrature from `fid-num` for arc length and fixed-iteration bisection for station parameters. No registry command; consumed by the edge comparison command.
>
> Constraints: KNP-1 (no FMA, `fid-num` facade, no std HashMap/HashSet), no panic/unwrap/unsafe. Tests: plate edge stations and frames, cylinder trim normals within 1e-12, gap error, closed-loop station count.

## Open questions

- Built in `fid-scan-edge::nominal` (PR #691). Choices made while building, open to review:
  - **Fixtures.** `fixtures/scan/edge_plate/body.txt` and `fixtures/scan/edge_cylinder_trim/body.txt` (plus `fixtures/scan/edge/nomedge/{hole,band,notch}.txt`) are small text B-rep descriptions loaded through `fid_geo_cad::BodyBuilder`, not STEP models: the kernel crate cannot (dev-)depend on the services-layer STEP importer. The cylinder trim curve is the ellipse cut by the plane 3x = 4z. Expected stations come from `fixtures/scan/edge/nomedge/make_nomedge_fixtures.py` (60-digit decimal, Romberg and Newton for the ellipse), CC0-1.0 project data.
  - **API.** `stations` returns `NominalStations` (stations plus the chain pieces in order with their reversal and length, the total length, the closed flag and the spacing) rather than a bare `Vec<Station>`. `CadResolver` only hands out a GEO.CAD `Body` (no assembly instance frames yet). `NominalEdge::Constructed` takes a `CurveDef` over a range with its face and an explicit `MaterialSide`, because KRN.CONS edge and trim-curve constructions are not built and a free curve carries no loop to fix the material side.
  - **Orientation.** The chain runs in the direction of the edge's coedge on the face (GEO.CAD keeps the material on the left seen from the outward normal), so `b = t x n` points away from the material; an edge used by no coedge of the face or by two (a seam) is `scan.edge.multi_face`.
  - **Corners and ends.** A station within `1e-12 m + 1e-12 L` of a junction takes the later piece's frame, at its start; on an open chain a station that far past the end sits at the end; on a closed chain no station lies within that distance of the full length.
  - **Frames.** The tangent is made exactly perpendicular to the face normal (one Gram-Schmidt step) after checking `|cos(t, n)| <= 1e-6`, and the cosine is reported per station; a station further than 1e-7 m (the GEO.CAD validation default) from the face surface is refused. Both limits are [unverified] and need evidence from imported CAD.
  - **Quadrature.** Five-point Gauss-Legendre nodes are constants in the crate (fid-num has no quadrature), adaptive from eight starting panels per piece, with typed errors when it does not converge.
  - Extra error codes beyond the three named in Design: `scan.edge.empty_chain`, `too_many_edges`, `unknown_entity`, `duplicate_edge`, `no_curve`, `bad_range`, `zero_length`, `chain_branch`, `arc_length_not_converged`, `eval`, `degenerate_tangent`, `foot`, `off_face`, `degenerate_normal`, `not_tangent`, `non_finite`. A spacing giving more than 1 000 000 stations is `scan.edge.bad_spacing`.

## Children

_Leaf: none._
