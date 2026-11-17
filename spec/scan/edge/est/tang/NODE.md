---
id: SCAN.EDGE.EST.TANG
title: Tangency-break edge method
level: 4
parent: SCAN.EDGE.EST
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.EDGE.EST-001, REQ-SCAN.EDGE.EST-002, REQ-SCAN.EDGE.EST-003, REQ-SCAN.EDGE.EST-004]
covers: [GOM.SRF.06, PW.CMP.04]
depends_on: [SCAN.EDGE.MESHBND, SCAN.EDGE.NOMEDGE, SCAN.EDGE.EST.EFIX, GEO.INDEX.KDTREE, GEO.INDEX.BVH, KRN.NUM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# SCAN.EDGE.EST.TANG: Tangency-break edge method

## Purpose

D-028 option A as a named `EdgeMethod`: the actual edge at a station is where the measured surface normal, walking outward across the edge in the station plane, first departs from the nominal surface normal by more than a threshold angle. It suits one-sided sheet scans, where no edge face is captured.

## Scope

**In:**
- Method ID `tangency_break` registered in the `extract_edge` method registry
- Clouds (with valid normals) and meshes (face normals) as actual
- Parameters: threshold angle θ, slab half-width w, persistence count k

**Out** (and which node owns it instead):
- The `EdgeMethod` interface and dispatcher (owned by `SCAN.EDGE.MESHBND`)
- Projected silhouette (owned by `SCAN.EDGE.EST.SILH`); default choice (owned by `SCAN.EDGE.EST.DEFS`)

## Requirements

- **REQ-SCAN.EDGE.EST.TANG-001** For each station (p, t, n, b) the method shall collect the actual samples within distance s of p and within the slab |t·(x − p)| ≤ w (default half the station spacing), order them by u = b·(x − p) ascending (ties by lowest index), and use each sample's measured unit normal (cloud normal or mesh face normal).
- **REQ-SCAN.EDGE.EST.TANG-002** The edge point shall be at the first sample i in that order that starts a run of k consecutive samples (default 3) whose angle to n exceeds θ (default 30° [unverified]), placed by linear interpolation in u between sample i − 1 and i on the angle crossing θ, and projected onto the station plane.
- **REQ-SCAN.EDGE.EST.TANG-003** If no such run exists, or fewer than k + 1 samples lie in the slab, then the hit shall be `NO_MATCH`.
- **REQ-SCAN.EDGE.EST.TANG-004** Clouds without valid normals for the slab samples shall have those samples skipped; if a cloud has no normals at all, the method shall fail with `scan.edge.normals_required`.
- **REQ-SCAN.EDGE.EST.TANG-005** Every hit shall record method ID `tangency_break`, version `1.0.0`, θ, w and k, and the method shall be determinism class D0.
- **REQ-SCAN.EDGE.EST.TANG-006** On the EFIX round and chamfer one-sided fixtures with σ = 10 µm, the trim component of the hit shall match the analytic tangency truth within 20 µm [unverified].

## Acceptance criteria

- **AC-1** Given `fixtures/scan/edge/est_round1_onesided` (σ = 0), θ = 30°, k = 3, every station hit lies within 1e-6 m of the EFIX tangency truth.
- **AC-2** Given the σ = 10 µm variant, REQ-006 holds at every station.
- **AC-3** Given a station inside a gap with no data, the hit is `NO_MATCH`.
- **AC-4** (error path) Given a cloud without a normal channel, the method fails with `scan.edge.normals_required`.
- **AC-5** Given 1 and 16 threads, hits are identical.

## Design

- **API** (`fid-scan-edge::est::tang`): `struct TangencyBreak; impl EdgeMethod for TangencyBreak { const ID = "tangency_break"; .. }`; params `{ theta_rad, slab_half_width_m?, persistence: u32 }`.
- **Algorithm:** cloud: k-d tree radius query (GEO.INDEX.KDTREE); mesh: BVH sphere query (GEO.INDEX.BVH) for triangles, sample = triangle centroid clipped to the slab with its face normal; angles by clamped dot and `fid-num` `acos`; no FMA.
- **Errors:** `scan.edge.normals_required`, `scan.edge.bad_parameter`.
- **Determinism:** D0.
- **Crate:** `fid-scan-edge` (kernel tier, KNP-1).

## Test fixtures

EFIX set `est_*_onesided` and `est_*_wrapped` (CC0).

## Build prompt

> In the Rust crate `fid-scan-edge`, module `est::tang`, implement `EdgeMethod` `tangency_break` and add it to the sorted method registry used by `extract_edge`. Per station (p, t, n, b): gather samples within search s of p and |t·(x − p)| ≤ w (cloud points with valid normals via the `fid-geo-index` k-d tree; mesh triangle centroids with face normals via `PrimitiveBvh`), sort by u = b·(x − p) then index, find the first run of k samples with angle(normal, n) > θ, interpolate in u between the previous sample and the run start at angle θ, project to the station plane and return it as `VALID`; otherwise `NO_MATCH`. Record method ID, version 1.0.0, θ, w and k. A cloud with no normals fails `scan.edge.normals_required`. Defaults θ = 30°, w = spacing/2, k = 3.
>
> Constraints: KNP-1 (no FMA, `fid-num` acos, total-order ties, no std HashMap/HashSet), §8. Tests: AC-1..AC-5 on the EFIX fixtures.

## Open questions

- D-028 asks for confirmation with at least two sheet-metal users; the default θ = 30° is a placeholder until then [unverified].

## Children

_Leaf: none._
