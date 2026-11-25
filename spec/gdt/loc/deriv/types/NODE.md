---
id: GDT.LOC.DERIV.TYPES
title: Derived-element type, dispatch and failure reasons
level: 4
parent: GDT.LOC.DERIV
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-gps]
built_pr: 621
satisfies: [REQ-GDT.LOC.DERIV-001, REQ-GDT.LOC.DERIV-002]
covers: []
depends_on: [GDT.RES.RESOLVE, GEO.FRAME]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.LOC.DERIV.TYPES: Derived-element type, dispatch and failure reasons

## Purpose

One type for every derived toleranced element (axis, median line, median surface, median points), one entry point `derive()` that routes the chain's reduction operator to the module that implements it, and one set of typed failure reasons. Every consumer (orientation, position, coaxiality, symmetry, derived straightness) calls `derive()` and gets the reduction operator's algorithm ID back for the ResultRecord.

## Scope

**In:** `DerivedElement { Axis{point, direction, extent}, MedianLine{points}, MedianSurface{points, normals?}, MedianPoints{points} }` with the producing algorithm ID; `derive(points, feature_kind, chain) -> Result<DerivedElement, DeriveError>`; a registry of implementations keyed by reduction-operator algorithm ID (axis from `GDT.LOC.DERIV.AXIS`; median line from `GDT.LOC.DERIV.MLINE`; median surface and points from `GDT.LOC.DERIV.MSURF`); `derivation_failed{reason}` with reasons `too_few_sections`, `too_few_points`, `fit_failed`, `no_opposite_point`.
**Out:** the derivations themselves (owned by `GDT.LOC.DERIV.AXIS`, `GDT.LOC.DERIV.MLINE`, `GDT.LOC.DERIV.MSURF`); tolerance evaluation (owned by the consumers).

## Requirements

- **REQ-GDT.LOC.DERIV.TYPES-001** `derive()` shall route the chain's reduction operator to the implementation registered for its algorithm ID and return the element together with that algorithm ID.
- **REQ-GDT.LOC.DERIV.TYPES-002** If no implementation is registered for the operator, then `derive()` shall return `UNSUPPORTED{alg}`.
- **REQ-GDT.LOC.DERIV.TYPES-003** If an implementation cannot produce the element from the data, then `derive()` shall return `derivation_failed{reason}` with one of the typed reasons and no element.
- **REQ-GDT.LOC.DERIV.TYPES-004** If the chain contains no reduction operator for a derived-element request, then `derive()` shall return `no_reduction_in_chain`.

## Acceptance criteria

- **AC-1** Given a registry with a stub axis implementation and a synthetic chain naming it, when `derive()` runs on fixture `fixtures/gdt/loc/deriv_stub_points.json`, then the stub's element is returned with its algorithm ID.
- **AC-2** (error path) Given a chain naming an unregistered operator, then `UNSUPPORTED{alg}`.
- **AC-3** (error path) Given a stub that fails with `too_few_sections`, then `derivation_failed{reason: too_few_sections}`.
- **AC-4** (error path) Given a chain with no reduction operator, then `no_reduction_in_chain`.
- **AC-5** The `DerivedElement` JSON Schema validates serialised examples of all four variants.

## Design

- **Registry command(s):** none (kernel API).
- **Data structures:** as in Scope; `DeriveRegistry: BTreeMap<AlgId, fn>` built at startup.
- **Algorithm:** find the reduction operator in the chain, look up, call.
- **Errors:** `UNSUPPORTED{alg}`, `derivation_failed{reason}`, `no_reduction_in_chain`.
- **Determinism:** D0.
- **Performance budget:** dispatch < 1 µs [unverified].
- **Crate:** `fid-gps` (module `loc::derive`).

## Test fixtures

Stub implementations and synthetic chains (CC0).

## Build prompt

> In crate `fid-gps` (kernel), module `loc::derive`, define `DerivedElement { Axis{point, direction, extent}, MedianLine{points}, MedianSurface{points, normals}, MedianPoints{points} }` carrying the producing algorithm ID, `DeriveError { Unsupported{alg}, DerivationFailed{reason}, NoReductionInChain }` with reasons `too_few_sections | too_few_points | fit_failed | no_opposite_point`, and `derive(points, feature_kind, chain)` that finds the chain's reduction operator and dispatches through a `BTreeMap<AlgId, fn>` registry. Export JSON Schemas. Tests per acceptance criteria with stubs. Constraints: CONSTITUTION §8, KNP-1, no std `HashMap`/`HashSet`.

## Open questions

- None.

## Children

_Leaf: none._
