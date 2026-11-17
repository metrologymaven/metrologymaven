---
id: GDT.DIM.SIZEISO.GLOBAL
title: Global sizes from a named association
level: 4
parent: GDT.DIM.SIZEISO
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.DIM.SIZEISO-004]
covers: []
depends_on: [KRN.LSQ, KRN.ENV, KRN.MZ]
uses_artifacts: [UNC.ORS.SIZE]
side_effect: R1
determinism: D3
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.DIM.SIZEISO.GLOBAL: Global sizes from a named association

## Purpose

A global size is one number for the whole feature: the size parameter of a feature associated to all its points (least squares, maximum inscribed, minimum circumscribed or minimax). Once the ISO row names the KRN association and which size parameter of its result to read, the computation is plain: run that fit and return the parameter. This leaf does exactly that, with no default association of its own.

## Scope

**In:** `global_size(points, feature_kind, op) -> GlobalSize` where `op` carries the KRN algorithm ID and the name of the size parameter to read (for example `diameter` or `width`); cylinders, circles, spheres and opposite parallel planes where KRN defines the fit; flags passed through.
**Out:** the fits (owned by `KRN.LSQ`, `KRN.ENV`, `KRN.MZ`); which association each ISO modifier uses (owned by DMT rows in `GDT.RES.ROWISO`); local, calculated and rank-order sizes (owned by sibling modules).

## Requirements

- **REQ-GDT.DIM.SIZEISO.GLOBAL-001** The function shall run exactly the KRN fit named by the operator and return the named size parameter in metres with the fit's algorithm ID.
- **REQ-GDT.DIM.SIZEISO.GLOBAL-002** If the KRN result has no parameter with the requested name, or KRN returns `method_undefined`, then the function shall return `UNSUPPORTED{alg, parameter}`.
- **REQ-GDT.DIM.SIZEISO.GLOBAL-003** If the fit carries `local_optimum_only` or a trust-region error, then the result shall carry the flag or fail with `fit_failed{reason}`.
- **REQ-GDT.DIM.SIZEISO.GLOBAL-004** Global sizes shall match every ORS-SIZE ISO global-size case within its stated tolerance.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/size/iso_cyl_lobed.json` (cylinder with 3-lobe form; known LS, MIC and MCC diameters), when `global_size` runs with each of the three fits, then each diameter matches its expected value within the fit's certified tolerance, and the three differ as constructed.
- **AC-2** (error path) Given an operator asking for parameter `length` from a circle fit, then `UNSUPPORTED{alg, parameter: length}`.
- **AC-3** Given a KRN stub flagging `local_optimum_only`, then the result carries the flag.
- **AC-4** (ORS) Every ORS-SIZE ISO global-size case passes within its tolerance (runs when published).

## Design

- **Registry command(s):** none (kernel function).
- **Data structures:** `GlobalSizeOp { alg: AlgId, parameter: SizeParam }`; `GlobalSize { value_m, alg, flags }`.
- **Algorithm:** call the KRN fit by algorithm ID; read the parameter by name from the typed fit result.
- **Errors:** `UNSUPPORTED{alg, parameter}`, `fit_failed{reason}`, `nan_input`.
- **Determinism:** D3 for iterative fits, D0 for closed-form ones.
- **Performance budget:** dominated by the fit [unverified].
- **Crate:** `fid-gps` (module `dim::size::iso::global`).

## Test fixtures

Seeded synthetic lobed cylinders and circles with analytically known associated sizes (CC0); ORS-SIZE suite from `UNC.ORS.SIZE`.

## Build prompt

> In crate `fid-gps`, module `dim::size::iso::global`, implement `global_size(points, feature_kind, op: GlobalSizeOp { alg, parameter }) -> Result<GlobalSize, SizeError>`: call exactly the KRN least-squares, envelope or minimum-zone fit named by `alg`, read the size parameter named by `parameter` from its typed result and return it in metres with the algorithm ID and flags. Unknown parameter or `method_undefined` gives `UNSUPPORTED{alg, parameter}`; trust-region failure gives `fit_failed{reason}`; propagate `local_optimum_only`. Choose no association yourself. Tests per acceptance criteria. Constraints: CONSTITUTION §8, KNP-1 (no FMA, fixed-order reductions, no std `HashMap`/`HashSet`, NaN is an error).

## Open questions

- None.

## Children

_Leaf: none._
