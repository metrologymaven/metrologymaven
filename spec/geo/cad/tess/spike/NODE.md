---
id: GEO.CAD.TESS.SPIKE
title: Time-boxed evaluation of an existing Rust B-rep crate as tessellator base
level: 4
parent: GEO.CAD.TESS
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-GEO.CAD.TESS-003]
covers: []
depends_on: [GEO.CAD.TOPO.MODEL]
side_effect: R1
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GEO.CAD.TESS.SPIKE: Time-boxed evaluation of an existing Rust B-rep crate as tessellator base

## Purpose

D-024 (accepted: option C, with option D evaluated in a time-boxed spike) puts tessellation in Rust and asks whether an existing Rust B-rep crate (for example `truck`, Apache-2.0 [unverified licence and maturity]) can serve as the starting point. This task runs that spike and publishes a short evaluation report that the EDGE, FACE and REFINE leaves read before implementation starts. It produces no product code.

## Scope

**In:**
- Candidate crates: at least `truck` and one other Rust tessellation or B-rep crate if one exists [unverified]
- Criteria: licence on the D-001 allowlist; KNP-1 audit (FMA use, hash containers, transcendental calls, SIMD dispatch, reduction order); `wasm32-unknown-unknown` build; no `unsafe` and no panics on the paths used, or the size of a fork that removes them; results on the GEO.CAD.TESS fixtures against REQ-GEO.CAD.TESS-001..003; effort estimate for fork versus write-from-scratch
- A time box of 5 agent-days [unverified]
- Report `docs/spikes/geo-cad-tess-base.md` in the implementation repository with a go/no-go recommendation

**Out** (and which node owns it instead):
- Adopting a dependency (a maintainer decision through the ECO.RULES allowlist process)
- Implementing the tessellator (owned by `GEO.CAD.TESS.EDGE`, `GEO.CAD.TESS.FACE`, `GEO.CAD.TESS.REFINE`, `GEO.CAD.TESS.BODY`)
- OCCT, which D-024 confines to an optional import plugin and never uses for tessellation

## Requirements

- **REQ-GEO.CAD.TESS.SPIKE-001** The spike shall finish within its time box and publish the report even if the outcome is "no-go".
- **REQ-GEO.CAD.TESS.SPIKE-002** The report shall state, per candidate, its SPDX licence, each KNP-1 rule found violated with file references, the wasm32 build result and the fixture results for vertex-on-surface error, chordal deviation and run-to-run bit-identity.
- **REQ-GEO.CAD.TESS.SPIKE-003** The report shall end with one recommendation (adopt and fork, borrow algorithms only, or write from scratch) and an effort estimate marked [unverified].
- **REQ-GEO.CAD.TESS.SPIKE-004** No candidate code shall be copied into project crates during the spike; experiments live in a throwaway branch.

## Acceptance criteria

- **AC-1** Given the time box, when it ends, then `docs/spikes/geo-cad-tess-base.md` exists with the four sections of REQ-002 and a recommendation.
- **AC-2** Given fixture `fixtures/geo/cad/tess/analytic_set` (cube, cylinder with hole, sphere, torus, as GEO.CAD.TOPO.MODEL descriptions), when each candidate tessellates it in the spike harness at chordal 1e-5 m, then the report lists measured maximum vertex-on-surface error and chordal deviation per body.
- **AC-3** Given two runs of each candidate on AC-2, then the report states whether outputs were bit-identical.

## Design

- **Registry command(s):** none; evaluation task.
- **Data structures:** a throwaway harness converting GEO.CAD.TOPO.MODEL bodies into each candidate's input.
- **Algorithm:** grep-based and manual KNP-1 audit; build matrix (three OSes plus wasm32); fixture runs with deviation measured by GEO.NURBS evaluation.
- **Errors:** not applicable.
- **Determinism:** N (an observed evaluation, not a value-producing function).
- **Performance budget:** time box 5 agent-days [unverified].
- **Crate:** none (throwaway branch); report in `docs/spikes/`.

## Test fixtures

- `fixtures/geo/cad/tess/analytic_set`: hand-written analytic bodies shared with GEO.CAD.TESS.BODY. CC-BY-4.0.

## Build prompt

> Run a time-boxed (5 agent-days) evaluation, in a throwaway branch, of existing Rust B-rep or tessellation crates (at least `truck`) as the base for the project's Rust tessellator (decision D-024: Rust owns tessellation; OCCT only as an optional import plugin). For each candidate record: SPDX licence versus the MIT/Apache-2.0 project licence and dependency allowlist; KNP-1 violations (FMA, hash containers, transcendental functions outside a facade, SIMD dispatch, unordered reductions) with file references; `wasm32-unknown-unknown` build result; presence of `unsafe` and panics on the needed paths; results on `fixtures/geo/cad/tess/analytic_set` at chordal 1e-5 m (max vertex-on-surface error, max chordal deviation, bit-identity across two runs). Do not copy candidate code into project crates. Publish `docs/spikes/geo-cad-tess-base.md` ending with one recommendation (adopt and fork, borrow algorithms only, write from scratch) and an [unverified] effort estimate. Acceptance: AC-1 report exists with all sections; AC-2 measured deviations listed; AC-3 bit-identity stated.

## Open questions

- If the recommendation is "adopt and fork", the dependency still needs maintainer approval through the ECO.RULES allowlist; that is a separate decision, not part of this task.

## Children

_Leaf: none._
