---
id: GDT.DIM.SIZEISO.DISP
title: ISO size evaluator, edition tag and unsupported refusal
level: 4
parent: GDT.DIM.SIZEISO
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.DIM.SIZEISO-001, REQ-GDT.DIM.SIZEISO-002]
covers: []
depends_on: [GDT.DIM.SIZEASME.CMD, GDT.DIM.SIZEISO.GLOBAL, GDT.DIM.SIZEISO.RANK, GDT.DIM.SIZEISO.LOCAL, GDT.DIM.SIZEISO.CALC, GDT.RES.RESOLVE]
side_effect: R1
determinism: D3
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.DIM.SIZEISO.DISP: ISO size evaluator, edition tag and unsupported refusal

## Purpose

The ISO path of `check.eval_size@1`: a `SizeEvaluator` registered under dialect `ISO` that refuses modifiers the DMT marks unsupported for the governing edition before computing anything, routes each operator in the resolved chain to the local, global, calculated or rank-order module, and tags the payload with the ISO 14405-1 edition from the governing spec.

## Scope

**In:** the ISO `SizeEvaluator` implementation and its registration; refusal on resolver `UNSUPPORTED`; routing by the chain's operator kinds (local size, global association, calculated size, rank-order statistic over local sizes); `IsoSizePayload { edition, modifier_key, value_si, local_count?, statistic?, alg_ids, flags }`.
**Out:** the command and registry (owned by `GDT.DIM.SIZEASME.CMD`); the computations (owned by `GDT.DIM.SIZEISO.LOCAL`, `GDT.DIM.SIZEISO.GLOBAL`, `GDT.DIM.SIZEISO.CALC`, `GDT.DIM.SIZEISO.RANK`); ISO rows (owned by `GDT.RES.ROWISO`).

## Requirements

- **REQ-GDT.DIM.SIZEISO.DISP-001** The ISO evaluator shall tag every payload with the ISO 14405-1 edition taken from the characteristic's governing spec.
- **REQ-GDT.DIM.SIZEISO.DISP-002** If the resolver returns `UNSUPPORTED` for the modifier and edition, then the evaluator shall return `UNSUPPORTED` with the row ID and shall call no computation.
- **REQ-GDT.DIM.SIZEISO.DISP-003** The evaluator shall route each chain operator to the module that owns its kind and, for a rank-order chain, pass the local sizes produced by the chain's local-size operator to the rank-order module.
- **REQ-GDT.DIM.SIZEISO.DISP-004** If the chain contains an operator kind no module implements, then the evaluator shall return `UNSUPPORTED{alg}`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/size/iso_gg_cylinder.json` with governing spec edition 2016 and a synthetic row naming a least-squares global size, when `fid check eval-size --char C --points P --json` runs, then the payload carries `edition: "2016"` and the global module's value.
- **AC-2** (error path) Given a synthetic row marked `unsupported` for edition 2025, then `UNSUPPORTED` with the row ID and a call-counting stub records zero computations.
- **AC-3** Given a synthetic rank-order row, then the rank module receives exactly the local sizes the local stub returned.
- **AC-4** (error path) Given a chain with an unknown operator kind, then `UNSUPPORTED{alg}`.

## Design

- **Registry command(s):** none of its own; registered into the `check.eval_size@1` evaluator registry under `ISO`.
- **Data structures:** `IsoSizePayload` as above.
- **Algorithm:** check resolver status, then iterate chain operators in order and dispatch by `Operator.kind` and algorithm ID.
- **Errors:** `UNSUPPORTED{row}`, `UNSUPPORTED{alg}`, pass-through module errors.
- **Determinism:** D3 when a global association is iterative, else D0; the payload records the class.
- **Performance budget:** dispatch overhead < 1 µs [unverified].
- **Crate:** `fid-gps` (module `dim::size::iso`).

## Test fixtures

Synthetic DMT rows with placeholder content (no standards text) and computation stubs (CC0).

## Build prompt

> In crate `fid-gps`, module `dim::size::iso`, implement the ISO `SizeEvaluator` and register it under dialect `ISO` in the `check.eval_size@1` evaluator registry. If the resolver status is `UNSUPPORTED`, return it with the row ID before any computation. Otherwise walk the chain operators in order and dispatch: local-size operators to the ISO local module, global associations to `global_size`, calculated sizes to the calculated module, rank-order statistics to `rank_statistic` over the local sizes from the chain's local operator; unknown kinds return `UNSUPPORTED{alg}`. Tag the payload with the ISO 14405-1 edition from the governing spec. Tests per acceptance criteria with stubs. Constraints: CONSTITUTION §8, KNP-1, no std `HashMap`/`HashSet`.

## Open questions

- None.

## Children

_Leaf: none._
