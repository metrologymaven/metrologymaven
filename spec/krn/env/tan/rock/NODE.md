---
id: KRN.ENV.TAN.ROCK
title: Rocking candidate sets, tie-break and tangential variant table
level: 4
parent: KRN.ENV.TAN
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-fit]
built_pr: 304
satisfies: [REQ-KRN.ENV.TAN-003, REQ-KRN.ENV.TAN-004]
covers: []
depends_on: [KRN.LSQ.TRAIT, KRN.SOLV.MSTART, KRN.NUM.RED, KRN.NUM.FN]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.ENV.TAN.ROCK: Rocking candidate sets, tie-break and tangential variant table

## Purpose

A datum feature that rocks (a slightly convex face on a surface plate) has more than one stable way to touch its simulator. Choosing one silently makes results depend on solver details, so this module collects every distinct stable contact set the tangential fits find, orders them by one deterministic rule and marks the selected one. It also owns the `TanVariant` and `MaterialSide` types and the table of defined (primitive, variant) pairs shared by the tangential fits.

## Scope

**In:** `TanVariant { ExternalMinimax, ConstrainedL2, ConstrainedL1 }` (required, no default); `MaterialSide` (an explicit outward direction or per-point normals); the defined-pairs table (plane, 2D line, cylinder with all three variants; 3D line, sphere, cone and others `method_undefined`); `CandidateSet { contacts: Vec<u32> sorted, objective, feature }`; deduplication of candidates from multi-start outcomes; additional candidates from hull neighbours of the optimum within `rock_tol`; the tie-break; the `RockReport`.
**Out:** the fits that produce candidates (`KRN.ENV.TAN.PLANE`, `KRN.ENV.TAN.LINE`, `KRN.ENV.TAN.CYL`); datum precedence and mobility (`GDT`).

## Requirements

- **REQ-KRN.ENV.TAN.ROCK-001** The module shall deduplicate candidate contact sets by their sorted contact indices and keep, for each distinct set, the best objective value and its feature.
- **REQ-KRN.ENV.TAN.ROCK-002** The module shall report as candidates every distinct stable set whose objective is within the required `rock_tol` (relative) of the best one, ordered by objective ascending, then by the lexicographic order of sorted contact indices.
- **REQ-KRN.ENV.TAN.ROCK-003** The first candidate in that order shall be marked `selected`, and the report shall state `rocking: true` whenever more than one candidate is listed.
- **REQ-KRN.ENV.TAN.ROCK-004** The module shall publish the defined-pairs table as data, and a lookup for an undefined (primitive, variant) pair shall return `method_undefined` naming both.
- **REQ-KRN.ENV.TAN.ROCK-005** For the same candidate inputs, the report shall be bit-identical on x86-64, ARM64 and wasm32 and independent of the order in which candidates were found.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/fit/tan-rock-v1` (a crowned plate with 4 symmetric high points, so 4 three-point contact sets with equal objective), when candidates from the plane fit's multi-start are passed in, then the report lists the 4 sets in the documented order, `selected` is the first and `rocking` is true.
- **AC-2** Given the same candidates shuffled, then the report is byte-identical.
- **AC-3** Given a flat plate with one stable set, then `rocking` is false and one candidate is listed.
- **AC-4** (error path) Given a lookup for (sphere, ConstrainedL1), then it returns `method_undefined{primitive: sphere, variant: constrained_l1}`.

## Design

- **Registry command(s):** none; library used by the tangential fits.
- **Data structures:** as in Scope; `RockReport { candidates: Vec<CandidateSet>, selected: u32, rocking: bool, rock_tol }`.
- **Algorithm:** sort candidates by (sorted contacts) for dedupe; keep minimum objective; filter by `rock_tol`; sort by (objective, contacts) with total-order comparisons.
- **Errors:** `MethodUndefined`, `InvalidParams{field: "rock_tol"}`.
- **Determinism:** D0.
- **Performance budget:** 1 000 candidates < 1 ms [unverified until measured].
- **Crate:** `fid-fit` (module `tan_rock`).

## Test fixtures

`fixtures/fit/tan-rock-v1`: synthetic crowned and flat plates generated in test code, CC-BY-4.0.

## Build prompt

> In crate `fid-fit` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-solv`), add module `tan_rock` with `TanVariant { ExternalMinimax, ConstrainedL2, ConstrainedL1 }` (required everywhere, no `Default`), `MaterialSide`, the defined-pairs table (plane, line2d, cylinder x all three variants; everything else `MethodUndefined`), `CandidateSet` and `fn rock_report(cands: &[CandidateSet], rock_tol: f64) -> Result<RockReport, FitError>`: dedupe by sorted contact indices, keep the best objective, keep sets within `rock_tol` of the best, order by (objective, contacts), mark the first `selected`, set `rocking` when more than one. Tests: `fixtures/fit/tan-rock-v1` crowned plate (4 candidates) and flat plate, shuffle invariance, undefined-pair lookup. KNP-1: no FMA, no `f64::min`/`max` (total-order compare), no HashMap/HashSet; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.ENV.TAN.ROCK.

## Open questions

- "Stable" is defined here as "a local optimum found by multi-start or a hull-neighbour set within `rock_tol`"; whether `GDT` needs a stricter, geometric stability test (centre of gravity inside the contact polygon) is for the `GDT` datum nodes to say.

## Children

_Leaf: none._
