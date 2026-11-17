---
id: KRN.MZ.EXT.UNDEF
title: Minimum-zone defined-pairs table and method_undefined
level: 4
parent: KRN.MZ.EXT
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-KRN.MZ.EXT-002]
covers: []
depends_on: [KRN.LSQ.TRAIT, KRN.MAN.CARD]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.MZ.EXT.UNDEF: Minimum-zone defined-pairs table and method_undefined

## Purpose

One table, served as data, that says for which primitives a minimum-zone fit is defined in this kernel and which algorithm implements each. Any other request returns a typed `method_undefined` error, so no caller ever receives a silently substituted method (for example a least-squares result labelled as minimum zone).

## Scope

**In:** the MZ table: 2D line, 3D line, plane, 2D circle, sphere, cylinder (`KRN.MZ.LIN`, `KRN.MZ.RND`), cone, slot, rectangle, ellipse, planar curve (`KRN.MZ.EXT` children), each with algorithm ID and `reference_basis` hint (`tracim`, `fiducial-ORS` or `none`, final value owned by `UNC`); undefined: point, 3D circle, torus, paraboloid, polyline, spline and anything else; the lookup used by the `fit.mz_*` dispatch; a CI check that every row has an implementation and a spec card.
**Out:** the fits (`KRN.MZ.LIN`, `KRN.MZ.RND`, other `KRN.MZ.EXT` children); reference-basis policy (`UNC`).

## Requirements

- **REQ-KRN.MZ.EXT.UNDEF-001** The module shall publish the MZ defined-pairs table as sorted static data and as canonical JSON with a digest.
- **REQ-KRN.MZ.EXT.UNDEF-002** If a minimum-zone fit is requested for a primitive not in the table, then the lookup shall return `method_undefined{primitive, method: mz}` and no algorithm.
- **REQ-KRN.MZ.EXT.UNDEF-003** CI shall fail if a table row names an algorithm without an implementation or spec card, or if a registered `fit.<primitive>.mz` algorithm is missing from the table.

## Acceptance criteria

- **AC-1** Given the table, when looked up for each of the 11 defined primitives, then it returns the expected algorithm IDs; for torus, it returns `method_undefined{primitive: torus, method: mz}`.
- **AC-2** Given the canonical JSON, then its digest equals the committed expected digest on x86-64, ARM64 and wasm32.
- **AC-3** Given a test registration of `fit.torus.mz` without a table row, then the CI check fails naming it.

## Design

- **Registry command(s):** none; used by the `fit.mz_*` commands.
- **Data structures:** `MzRow { primitive, algorithm: AlgorithmId, reference_basis_hint }`, sorted by primitive.
- **Errors:** `FitError::MethodUndefined{primitive, method}`.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-fit` (module `mz_table`).

## Test fixtures

None beyond the committed expected digest.

## Build prompt

> In crate `fid-fit` (pure Rust, no I/O, no `unsafe`), add module `mz_table` with a sorted static table of `MzRow { primitive, algorithm, reference_basis_hint }` for line2d, line3d, plane, circle2d, sphere, cylinder, cone, slot, rectangle, ellipse and curve2d, a lookup returning `FitError::MethodUndefined{primitive, method: mz}` for anything else, canonical JSON with a digest, and a CI test that every row has an implementation and spec card and every registered `fit.*.mz` algorithm has a row. Tests: lookups for all 11 rows and torus, digest identity on x86-64/ARM64/wasm32, missing-row detection. Deny `unwrap`/`expect`/`panic`; no HashMap/HashSet. Acceptance: AC-1..AC-3 of spec node KRN.MZ.EXT.UNDEF.

## Open questions

- Whether a 3D circle (roundness in a plane fitted from 3D data) should be a defined pair or always a projection to `circle2d` by the caller is for `GDT` to say; it is undefined here.

## Children

_Leaf: none._
