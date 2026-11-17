---
id: GDT.PROF.ZDEF.UNIF
title: Constant-width zone intervals and dispatch
level: 4
parent: GDT.PROF.ZDEF
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.PROF.ZDEF-001, REQ-GDT.PROF.ZDEF-002, REQ-GDT.PROF.ZDEF-003]
covers: []
depends_on: [GDT.PROF.DEV, GDT.ZONE.ZTYPE, GDT.RES.RESOLVE]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.PROF.ZDEF.UNIF: Constant-width zone intervals and dispatch

## Purpose

Owns the `ZoneDefinition` type for profile tolerances and turns the constant-width definitions (uniform bilateral, unilateral outward or inward, unequally disposed with a stated share) into one allowed interval on the signed deviation from `GDT.PROF.DEV`, plus the equivalent disposition shift that `GDT.ZONE.ZTYPE`'s offset shape takes. It also dispatches: definitions the row marks unsupported, and non-uniform definitions until `GDT.PROF.ZDEF.NONUNIF` lands, return `UNSUPPORTED`. Which side a stated share applies to is read from the resolved chain, never hard-coded, so the interpretation stays in the DMT.

## Scope

**In:** `ZoneDefinition { Bilateral{t}, Unilateral{t, side}, Unequal{t, share, share_side}, NonUniform{..} }`; interval construction; disposition shift; the `zone_definition` record fragment for the ResultRecord; dispatch and `UNSUPPORTED`.
**Out:** non-uniform interval construction (owned by `GDT.PROF.ZDEF.NONUNIF`); deviations (owned by `GDT.PROF.DEV`); zone usage and value (owned by `GDT.ZONE.ZTYPE`, `GDT.PROF.LINE`, `GDT.PROF.SURF`); the meaning of a share for a callout (owned by DMT rows in `GDT.RES.ROWASME` and `GDT.RES.ROWISO`).

## Requirements

- **REQ-GDT.PROF.ZDEF.UNIF-001** For `Bilateral{t}`, the module shall return the interval [−t/2, +t/2] on the signed deviation (positive away from material, as defined by `GDT.PROF.DEV`).
- **REQ-GDT.PROF.ZDEF.UNIF-002** For `Unilateral{t, side}`, the module shall return [0, t] when the side is outward and [−t, 0] when the side is inward.
- **REQ-GDT.PROF.ZDEF.UNIF-003** For `Unequal{t, share, share_side}`, the module shall return [−(t − share), share] when the chain's `share_side` is outward and [−share, t − share] when it is inward.
- **REQ-GDT.PROF.ZDEF.UNIF-004** Each constant interval shall be returned with its width (upper − lower) and its disposition shift c = (lower + upper)/2, so that the zone contains a deviation d exactly when |d − c| ≤ width/2.
- **REQ-GDT.PROF.ZDEF.UNIF-005** If t ≤ 0, share < 0, share > t, or any parameter is NaN, then the module shall return `bad_zone_parameter` naming the parameter.
- **REQ-GDT.PROF.ZDEF.UNIF-006** If the resolved row lists the stated definition kind as unsupported (a limitation code `zone_def_unsupported:<kind>`), or the definition is `NonUniform` and no non-uniform implementation is linked, then the module shall return `UNSUPPORTED` with the definition kind and no interval.
- **REQ-GDT.PROF.ZDEF.UNIF-007** The module shall produce a `ZoneDefRecord { kind, t, share, share_side, lower, upper, shift }` for the ResultRecord payload, with all lengths in metres.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/prof/zdef/constant.json` (t = 0.4 mm; bilateral, unilateral outward and inward, unequal share 0.1 mm outward and inward), when `zone_intervals` runs, then the intervals are [−0.2, 0.2], [0, 0.4], [−0.4, 0], [−0.3, 0.1] and [−0.1, 0.3] mm exactly as f64 computed from the dyadic-scaled inputs in the fixture, and each shift equals (lower + upper)/2 bit-for-bit.
- **AC-2** Given 10 000 seeded (t, share, d) triples (seed `0x2def`), then `lower ≤ d ≤ upper` agrees with `|d − shift| ≤ width/2` in every case where both sides are exactly representable, and disagreements on the remainder are listed and none exceeds 1 ulp of t.
- **AC-3** (error path) Given share = 0.5 mm with t = 0.4 mm, then `bad_zone_parameter{share}`; given t = NaN, then `bad_zone_parameter{t}`.
- **AC-4** (error path) Given a synthetic row with limitation `zone_def_unsupported:unequal` and an unequal definition, then `UNSUPPORTED{unequal}` and no interval.
- **AC-5** Given a `NonUniform` definition in a build without the non-uniform module, then `UNSUPPORTED{non_uniform}`.
- **AC-6** (determinism) The fixture outputs are bit-identical on all CI platforms.

## Design

- **Registry command(s):** none (library for `check.eval_line_profile@1` and `check.eval_surface_profile@1`).
- **Data structures:** `ZoneDefinition` as in Scope; `Side { Outward, Inward }`; `Interval { lower_m, upper_m }`; `ConstantZone { interval, width_m, shift_m }`; `ZoneDefRecord` as in REQ-007; `ZoneIntervals { Constant(ConstantZone) | PerPoint(Vec<Interval>) }` (the second variant is filled by `NONUNIF`).
- **Algorithm:** validate; build the interval per REQ-001..003; width = upper − lower; shift = (lower + upper) · 0.5; no other arithmetic.
- **Errors:** `bad_zone_parameter{name}`, `UNSUPPORTED{kind}`.
- **Determinism:** D0.
- **Performance budget:** trivial (constant time per characteristic).
- **Crate:** `fid-gps` (module `prof::zdef`).

## Test fixtures

`fixtures/gdt/prof/zdef/constant.json` and seeded property cases, generated in-repo (CC0). Synthetic DMT rows from `fixtures/gdt/dmt/synthetic_table_v1.json` only; no standards text.

## Build prompt

> In crate `fid-gps` (kernel, pure, KNP-1), add module `prof::zdef`. Define `ZoneDefinition { Bilateral{t}, Unilateral{t, side}, Unequal{t, share, share_side}, NonUniform{..} }`, `Side { Outward, Inward }` (outward = positive signed deviation, i.e. away from material), `Interval`, `ConstantZone { interval, width_m, shift_m }`, `ZoneIntervals { Constant | PerPoint }` and `ZoneDefRecord { kind, t, share, share_side, lower, upper, shift }`. Implement `zone_intervals(def, chain_params, row_limitations) -> Result<ZoneIntervals, ZdefError>`: bilateral [−t/2, t/2]; unilateral outward [0, t], inward [−t, 0]; unequal with share on the outward side [−(t−share), share], on the inward side [−share, t−share] (the side comes from the resolved chain parameter `share_side`, never hard-coded); shift = (lower + upper)·0.5, width = upper − lower. Errors: `bad_zone_parameter{name}` for t ≤ 0, share < 0, share > t, NaN; `UNSUPPORTED{kind}` when the row carries limitation `zone_def_unsupported:<kind>` or for `NonUniform` while no non-uniform implementation is linked. Tests per the acceptance criteria (exact intervals, 10 000-case seeded property test, error paths, cross-platform bit identity). Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`; all input hostile), KNP-1 (no FMA, fixed-order arithmetic, NaN is an error), no standards text in code, comments or fixtures.

## Open questions

- Whether the ASME unequally disposed share is measured on the side that adds material or removes it is DMT content; this leaf only takes `share_side` from the chain.

## Children

_Leaf: none._
