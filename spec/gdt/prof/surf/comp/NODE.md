---
id: GDT.PROF.SURF.COMP
title: Composite and multi-segment profile tiers
level: 4
parent: GDT.PROF.SURF
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.PROF.SURF-002, REQ-GDT.PROF.SURF-004]
covers: [PW.GDT.05]
depends_on: [GDT.PROF.SURF.PROB, GDT.ZONE.COMP]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.PROF.SURF.COMP: Composite and multi-segment profile tiers

## Purpose

Turns a composite or multi-segment profile frame into one profile problem per tier (each tier with its own tolerance value, zone definition and datum references) and hands them to `GDT.ZONE.COMP`, which applies the DOF constraints the resolved row assigns to each tier. It returns one segment result per tier in frame order. The tier semantics (which DOFs a lower tier releases) are DMT content held in `GDT.ZONE.COMP`'s input, not here.

## Scope

**In:** reading the tiers of a profile frame from the characteristic; building a `ProfileZoneProblem` per tier through `GDT.PROF.SURF.PROB`; passing the tier set to `solve_composite`; collecting per-tier results in frame order.
**Out:** tier constraint semantics and the tier solve (owned by `GDT.ZONE.COMP`); single-segment construction (owned by `GDT.PROF.SURF.PROB`); payload and command (owned by `GDT.PROF.SURF.CMD`).

## Requirements

- **REQ-GDT.PROF.SURF.COMP-001** When a profile frame has more than one tier, the module shall build one profile problem per tier with that tier's tolerance, zone definition and datum references, and shall share the toleranced face set and points across tiers.
- **REQ-GDT.PROF.SURF.COMP-002** The module shall return one segment result per tier, in frame order, each carrying the tier index and the DOFs `GDT.ZONE.COMP` held fixed.
- **REQ-GDT.PROF.SURF.COMP-003** If `GDT.ZONE.COMP` returns `UNSUPPORTED` for the frame, then the module shall return `UNSUPPORTED` for the whole characteristic and no tier values.
- **REQ-GDT.PROF.SURF.COMP-004** Per-tier values shall match every ORS-GDT composite-profile case within its stated tolerance.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/prof/surf/composite_two_tier.json` (plane-and-cylinder part, upper tier t = 0.5 mm to datums A|B|C, lower tier t = 0.1 mm to A, synthetic row: lower tier holds only the DOFs datum A constrains; points with a planted 0.15 mm translation and 0.04 mm form), when evaluated, then tier 0 value = 2·(0.15 + 0.02) mm within 1e-9 m and tier 1 value = 0.04 mm within 1e-9 m.
- **AC-2** Given a multi-segment frame with the same tiers but a synthetic row marking the tiers independent, then each tier equals the single-segment result for its own datums.
- **AC-3** (error path) Given a synthetic row assigning no constraint set to tier 1, then `UNSUPPORTED` and no tier values.
- **AC-4** (determinism) AC-1 gives bit-identical tier results on all CI platforms.

## Design

- **Registry command(s):** none (library for `check.eval_surface_profile@1`).
- **Data structures:** `ProfileTier { index, tolerance_m, zone_def, datums }`; `TierSegment { index, held: DofSet, segment: SegmentResult }`.
- **Algorithm:** read tiers; for each tier build the problem with `prof::surf::problem`; call `zone::comp::solve_composite(frame, tier_problems, row)`; map tier solutions back to segment results (recomputed value and extremes at each tier pose).
- **Errors:** `UNSUPPORTED`, pass-through from PROB and the composite solver.
- **Determinism:** D0 when every tier solve certifies.
- **Performance budget:** two tiers on 100 000 points < 2 s [unverified].
- **Crate:** `fid-gps` (module `prof::surf::composite`).

## Test fixtures

Synthetic two-tier part with planted translation and form (CC0, in-repo); synthetic rows only; ORS-GDT composite-profile cases when published.

## Build prompt

> In crate `fid-gps` (kernel, KNP-1), add module `prof::surf::composite`. Implement `solve_profile_tiers(char, points, drfs, chain, row) -> Result<Vec<TierSegment>, ProfError>`: read the frame's tiers (tolerance, zone definition, datums); build one `ProfileZoneProblem` per tier with `prof::surf::problem`, sharing faces and points; call `zone::comp::solve_composite`; return one `TierSegment { index, held, segment }` per tier in frame order with value and extremes recomputed at each tier pose. If the composite solver returns `UNSUPPORTED`, return `UNSUPPORTED` with no tier values. Tests per the acceptance criteria (two-tier analytic fixture, multi-segment independence, unsupported path, cross-platform bit identity). Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, fixed-order reductions, no std `HashMap`/`HashSet`, NaN is an error); no tier semantics hard-coded (they come from the row).

## Open questions

- AC-1's expected values assume the synthetic row's tier semantics; real expected values come from ORS-GDT cases authored under D-009.

## Children

_Leaf: none._
