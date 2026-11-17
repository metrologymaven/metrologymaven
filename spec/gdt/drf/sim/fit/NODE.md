---
id: GDT.DRF.SIM.FIT
title: Single-feature datum simulation with precedence constraints
level: 4
parent: GDT.DRF.SIM
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.DRF.SIM-001, REQ-GDT.DRF.SIM-004]
covers: [GOM.GDT.14]
depends_on: [KRN.ENV, GDT.RES.RESOLVE, GEO.FRAME]
uses_artifacts: [UNC.ORS.DRF]
side_effect: R1
determinism: D3
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.DRF.SIM.FIT: Single-feature datum simulation with precedence constraints

## Purpose

Turns one measured planar or cylindrical datum feature into a simulated datum (plane or axis) by running exactly the tangential fit the resolved DMT row names (external minimax, constrained L2 or constrained L1 outer-tangent, from `KRN.ENV.TAN`), while holding the degrees of freedom that higher-precedence datums have already fixed. The list of already-fixed DoFs is read from the DoF table of the DRF under construction (owned by `GDT.DRF.BUILD`), so there is one DoF table in the system. Which fit the row names is the row's business; this leaf only executes it.

## Scope

**In:** `simulate(feature_points, feature_kind, chain, fixed: FixedDofs) -> SimulatedDatum`; translating the fixed DoFs and the nominal relationship to the higher datums (from the characteristic's nominal geometry) into orientation and location constraints for the KRN fit; passing KRN's certificate and contact set through; reporting the simulator algorithm ID; returning the KRN candidate list untouched for `GDT.DRF.SIM.ROCK`.
**Out:** the fits (owned by `KRN.ENV`); candidate filtering and cap (owned by `GDT.DRF.SIM.ROCK`); choosing among candidates (owned by `GDT.DRF.SIM.SELECT`); DoF bookkeeping and DRF assembly (owned by `GDT.DRF.BUILD`); common datums and targets (owned by `GDT.DRF.COMMON`); the fit choice (owned by DMT rows via `GDT.RES`).

## Requirements

- **REQ-GDT.DRF.SIM.FIT-001** The simulator shall call exactly the KRN tangential fit named by the resolved chain, with the variant parameter from the chain and no default, and record the fit's algorithm ID and certificate in the `SimulatedDatum`.
- **REQ-GDT.DRF.SIM.FIT-002** The simulator shall hold every DoF listed as fixed by higher-precedence datums at its simulated value, so the simulated datum's change in those DoFs is at most 1e-12 m and 1e-12 rad relative to the higher datums.
- **REQ-GDT.DRF.SIM.FIT-003** If the KRN fit cannot accept the required constraint for that primitive, then the simulator shall return `constraint_unsupported{alg, dof}` and no simulated datum.
- **REQ-GDT.DRF.SIM.FIT-004** If KRN returns `method_undefined`, then the simulator shall return `UNSUPPORTED{alg}`.
- **REQ-GDT.DRF.SIM.FIT-005** For single-candidate ORS-DRF cases within P1a scope (planar and cylindrical, RMB), simulated datums shall match within the case's stated tolerance.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/drf/sim_plane_bumpy.json` (planar feature with known outer-tangent plane under each variant) and a chain naming the external-minimax variant, when simulated as primary, then the plane matches the expected plane within 1e-12 m and the record carries the algorithm ID and certificate.
- **AC-2** Given fixture `sim_secondary_plane.json` with a primary datum fixing {tz, rx, ry}, when the secondary is simulated, then its normal stays perpendicular to the primary normal within 1e-12 rad.
- **AC-3** (error path) Given a KRN stub that rejects an orientation constraint for cylinders, then `constraint_unsupported{alg, dof: rx}`.
- **AC-4** (error path) Given a chain naming an undefined variant and primitive pair, then `UNSUPPORTED{alg}`.
- **AC-5** (ORS) Every single-candidate ORS-DRF P1a case passes when the suite is published.

## Design

- **Registry command(s):** none. `ALN.DATUM.SYS` and other callers reach simulation only through the R1 dry-run of `gdt.build_drf@1` (owned by `GDT.DRF.BUILD`), which calls this function.
- **Data structures:** `FixedDofs { set: DofSet, values: Pose }` (from the DRF under construction); `SimulatedDatum { Plane{point, normal} | Axis{point, direction}, alg, certificate, candidates }` (shared type with `GDT.DRF.BUILD`).
- **Algorithm:** map fixed DoFs to KRN constraint terms (orientation: fixed normal or axis direction components; location: fixed offsets), call the fit, verify constraint residuals from the certificate.
- **Errors:** `constraint_unsupported{alg, dof}`, `UNSUPPORTED{alg}`, `nan_input`, pass-through KRN errors.
- **Determinism:** D3 (iterative tangential fits), recorded in the result.
- **Performance budget:** dominated by the KRN fit [unverified].
- **Crate:** `fid-gps` (module `drf::sim`).

## Test fixtures

Seeded synthetic planar and cylindrical datum features with analytically known tangent elements (CC0); ORS-DRF suite from `UNC.ORS.DRF`.

## Build prompt

> In crate `fid-gps` (kernel), module `drf::sim`, implement `simulate(points, feature_kind, chain, fixed: FixedDofs) -> Result<SimulatedDatum, SimError>`. Call exactly the KRN tangential fit (`fit.tan_plane`, `fit.tan_cylinder`, `fit.tan_line` family) and variant the chain names; never default a variant. Hold every DoF in `fixed` (taken from the DRF's DoF table, not recomputed) by passing orientation and location constraints to the fit; if KRN cannot accept a needed constraint return `constraint_unsupported{alg, dof}`; `method_undefined` becomes `UNSUPPORTED{alg}`. Record algorithm ID, certificate and the KRN candidate list unchanged. Tests per acceptance criteria. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, `fid-num` transcendentals, fixed-order reductions, no std `HashMap`/`HashSet`, NaN is an error).

## Open questions

- `KRN.ENV.TAN` does not yet state that its fits accept orientation and location constraints from higher datums; this leaf needs that (raised for KRN).

## Children

_Leaf: none._
