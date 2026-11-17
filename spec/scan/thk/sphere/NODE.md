---
id: SCAN.THK.SPHERE
title: Inscribed-sphere wall thickness method
level: 3
parent: SCAN.THK
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.THK-003, REQ-SCAN.THK-005, REQ-SCAN.THK-006, REQ-SCAN.THK-007]
covers: [GOM.SRF.07]
depends_on: [SCAN.THK.RAY, SCAN.THK.TFIELD, GEO.INDEX, GEO.DIST]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# SCAN.THK.SPHERE: Inscribed-sphere wall thickness method

## Purpose

The alternative wall-thickness definition: at each surface point, the diameter of the largest sphere inside the material that touches the surface at that point. Unlike a ray, it is not fooled by a ray that slips past a thin rib at a grazing angle, and it gives the local "fit-through" thickness that casting and moulding users often expect [unverified]. It is a method of `compare.thickness_wall@1` (owned by `SCAN.THK.RAY`), selected with `method = sphere`.

## Scope

**In:**
- Kernel function `sphere_thickness(source, params) -> ThicknessField` dispatched by `compare.thickness_wall@1`
- Shrinking-ball iteration per point with a fixed iteration cap
- Maximum thickness, convergence tolerance and `NO_MATCH` handling
- Exclusion of synthesized triangles as touching surfaces

**Out** (and which node owns it instead):
- The command, parameter parsing and the ray method (owned by `SCAN.THK.RAY`)
- Field element, limits and statistics (owned by `SCAN.THK.TFIELD`)

## Requirements

- **REQ-SCAN.THK.SPHERE-001** For each start point p with outward unit normal n, the method shall find the largest radius r such that the open ball centred at p − r·n contains no surface point, and shall report thickness 2r in metres.
- **REQ-SCAN.THK.SPHERE-002** The radius shall be found by the shrinking-ball iteration: start at r₀ = half the maximum thickness, replace r by the radius of the sphere through p, tangent at p, that passes through the nearest surface point to the current centre, and stop when r changes by less than the stated tolerance (default 1e-9 m) or after 64 iterations.
- **REQ-SCAN.THK.SPHERE-003** If 2r exceeds the maximum thickness after convergence, or the iteration does not converge within 64 iterations, then the point shall be `NO_MATCH` with no value.
- **REQ-SCAN.THK.SPHERE-004** Nearest-point queries shall exclude the start point's incident triangles within ε of p and all synthesized triangles.
- **REQ-SCAN.THK.SPHERE-005** The method shall be determinism class D0 and shall record definition ID `thk.sphere.v1` with the field.
- **REQ-SCAN.THK.SPHERE-006** On the stepped-plate fixture the method shall report the known thicknesses within 1 µm at points at least 3 mm from a step [unverified tolerance].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/thk_step_plate` (seed 59), when `fid compare thickness-wall --method sphere --max-thickness 0.01` runs, then points ≥ 3 mm from a step report 2.000, 3.000 and 4.000 mm within 1 µm.
- **AC-2** Given fixture `fixtures/scan/thk_sphere_shell` (hollow sphere, outer radius 50 mm, inner 47 mm), every outer-surface point reports 3.000 mm ± 1 µm.
- **AC-3** Given fixture `fixtures/scan/thk_rib` (a 1 mm rib on a 5 mm base), points on the rib flank report 1.0 mm ± 2 µm while the ray method at 30° tilt over-reports; the test documents both.
- **AC-4** Given `--max-thickness 0.0025` on the stepped plate, the 3 and 4 mm regions are `NO_MATCH`.
- **AC-5** Given the same input on 1 and 16 threads, field digests are identical.

## Design

- **API:** `fid_scan_thk::sphere::sphere_thickness(source: &SurfaceView, params: &WallParams, out: &mut ThicknessFieldBuilder) -> Result<(), ThkError>`; no registry command of its own (class R1 for the pure computation; the owning command is W2).
- **Algorithm:** shrinking-ball medial-axis approximation (Ma, Bae, Choi, 2012) [unverified citation detail]: centre c = p − r·n; q = nearest surface point to c (GEO.DIST, excluding per REQ-004, ties lowest entity ID); new r = |p − q|² / (2·n·(p − q)) evaluated without FMA; terminate per REQ-002.
- **Errors:** `scan.thk.no_normals`, `scan.thk.bad_parameter`.
- **Determinism:** D0 (fixed iteration cap, total-order tie-breaks).
- **Performance budget:** 10⁶ points within 60 s [unverified].
- **Crate:** `fid-scan-thk`.

## Test fixtures

- `fixtures/scan/thk_step_plate` (shared), `fixtures/scan/thk_sphere_shell`, `fixtures/scan/thk_rib`: generated, CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-scan-thk`, module `sphere`, implement `sphere_thickness(source, params, out)`, the `sphere` method of the existing `compare.thickness_wall@1` command. For each start point p with outward normal n: shrinking-ball iteration starting at r0 = max_thickness/2; centre c = p − r·n; q = nearest surface point to c via the GEO.DIST API, excluding triangles incident to p within ε and synthesized triangles; r_new = |p − q|² / (2·n·(p − q)) (no FMA); stop when |r_new − r| < tolerance (default 1e-9 m) or after 64 iterations; thickness = 2r; NO_MATCH if not converged or 2r > max thickness. Write through `ThicknessFieldBuilder` with definition ID `thk.sphere.v1`. Pure, D0, CPU only.
>
> Constraints: KNP-1 (no FMA, `fid-num` facade, no std HashMap/HashSet, total-order ties), no panic/unwrap/unsafe. Tests: stepped plate within 1 µm away from steps, hollow sphere shell 3 mm, rib flank comparison with the ray method, max-thickness NO_MATCH, thread invariance.

## Open questions

- None.

## Children

_Leaf: none._
