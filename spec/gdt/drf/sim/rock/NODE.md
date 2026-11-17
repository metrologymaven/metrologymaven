---
id: GDT.DRF.SIM.ROCK
title: Rocking candidate sets within a tie tolerance
level: 4
parent: GDT.DRF.SIM
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.DRF.SIM-002, REQ-GDT.DRF.SIM-003, REQ-GDT.DRF.SIM-004]
covers: []
depends_on: [GDT.DRF.SIM.FIT]
uses_artifacts: [UNC.ORS.DRF]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.DRF.SIM.ROCK: Rocking candidate sets within a tie tolerance

## Purpose

A rocking datum feature has several stable contact sets that are equally valid within a tie tolerance. KRN already enumerates candidate contact sets; this leaf filters them by the tie tolerance the resolved row states, orders them deterministically, enforces the candidate cap the row states, and hands the list to the selection step. Choosing among candidates is not done here.

## Scope

**In:** filtering KRN candidates whose objective lies within the row's tie tolerance of the best; deterministic ordering (objective, then lexicographic contact indices); cap from the row; `rocking_unbounded` when the cap is exceeded; `CandidateSet` recorded in the simulated datum.
**Out:** generating candidates (owned by `KRN.ENV.TAN` through `GDT.DRF.SIM.FIT`); applying a selection rule (owned by `GDT.DRF.SIM.SELECT`); tie tolerance and cap values (owned by DMT rows).

## Requirements

- **REQ-GDT.DRF.SIM.ROCK-001** The function shall keep exactly the KRN candidates whose objective is within the chain's tie tolerance of the best objective and order them by objective and then by sorted contact indices.
- **REQ-GDT.DRF.SIM.ROCK-002** If the kept count exceeds the chain's cap, then the function shall return `rocking_unbounded{count, cap}` and no candidate set.
- **REQ-GDT.DRF.SIM.ROCK-003** If the chain supplies no tie tolerance or no cap, then the function shall return `rocking_params_missing` rather than assume values.
- **REQ-GDT.DRF.SIM.ROCK-004** Candidate sets shall match the candidate lists of every ORS-DRF rocking case within the case's stated tolerance.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/drf/rock_convex_plate.json` (a slightly convex plate with four symmetric contact sets of equal objective), when filtered with tie tolerance 1e-9 m and cap 8, then four candidates are kept in the specified order.
- **AC-2** Given cap 3 on the same fixture, then `rocking_unbounded{count: 4, cap: 3}`.
- **AC-3** Given candidates in shuffled order from a KRN stub, then the output order is unchanged.
- **AC-4** (error path) Given a chain without a cap, then `rocking_params_missing`.

## Design

- **Registry command(s):** none (kernel function `filter_candidates(krn_candidates, chain) -> Result<CandidateSet, RockError>`).
- **Data structures:** `CandidateSet { candidates: [Candidate { contact: [u32], objective, datum }], tie_tolerance, cap }`.
- **Algorithm:** total-order min, filter, stable sort by (objective bits, contact list).
- **Errors:** `rocking_unbounded{count, cap}`, `rocking_params_missing`.
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-gps` (module `drf::sim::rock`).

## Test fixtures

Synthetic rocking plates and KRN candidate stubs (CC0); ORS-DRF rocking cases from `UNC.ORS.DRF`.

## Build prompt

> In crate `fid-gps`, module `drf::sim::rock`, implement `filter_candidates(krn_candidates, chain) -> Result<CandidateSet, RockError>`: read tie tolerance and cap from the chain (missing gives `rocking_params_missing`; never default), keep candidates with objective within the tolerance of the total-order minimum, sort by objective then sorted contact indices, and return `rocking_unbounded{count, cap}` above the cap. Do not choose a candidate. Tests per acceptance criteria. Constraints: CONSTITUTION §8, KNP-1 (total-order min/max, no std `HashMap`/`HashSet`, NaN is an error).

## Open questions

- None.

## Children

_Leaf: none._
