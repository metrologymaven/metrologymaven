---
id: GDT.ZONE.COMP
title: Composite feature-control-frame tiers
level: 3
parent: GDT.ZONE
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.ZONE-004, REQ-GDT.ZONE-007]
covers: [PCD.DIM.03, PW.GDT.09]
depends_on: [GDT.ZONE.GROUP.PATTERN, GDT.ZONE.MATL.BOTH, GDT.ZONE.POSE.SOLVE, GDT.RES.RESOLVE, GDT.DRF.BUILD]
uses_artifacts: [UNC.ORS.GDT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.ZONE.COMP: Composite feature-control-frame tiers

## Purpose

Evaluates composite feature control frames (composite position and composite profile): the same zone problem solved once per tier with tier-specific constraints, for example the pattern-locating tier using all datum DOFs and the feature-relating tier constrained only in orientation to the referenced datums (concept doc 05 §2.5 example row). Which DOFs each tier keeps is DMT content; this leaf reads each tier's constraint set from the resolved row, builds the tier problem on the existing pose, group and material-condition solvers, and reports one value per tier. Multi-segment (stacked single-segment) frames are handled as independent tiers.

## Scope

**In:** per-tier problem construction from the row's tier constraint sets; the free DOFs of each tier as (DRF free DOFs) ∪ (DOFs the row releases for that tier); dispatch to the single-feature solve, the pattern solve or the material-condition solve as the tier requires; one `TierSolution` per tier with index, held DOFs and pose; `UNSUPPORTED` for tiers without a constraint set.
**Out:** tier constraint definitions (DMT rows in `GDT.RES.ROWASME` and `GDT.RES.ROWISO`); profile-specific tier payloads (owned by `GDT.PROF.SURF.COMP`); position payloads (owned by `GDT.LOC.POS`).

## Requirements

- **REQ-GDT.ZONE.COMP-001** When a composite frame is evaluated, the engine shall solve each tier separately with the DOF constraints the resolved row assigns to that tier and report one value per tier.
- **REQ-GDT.ZONE.COMP-002** Each tier result shall record its tier index, the DOFs it held fixed, and the pose found.
- **REQ-GDT.ZONE.COMP-003** If the row assigns no constraint set to a tier present in the frame, then the engine shall return `UNSUPPORTED` for the whole frame.
- **REQ-GDT.ZONE.COMP-004** The engine shall reproduce every ORS-GDT composite case within its stated tolerance.
- **REQ-GDT.ZONE.COMP-005** For a tier with several features, the engine shall solve the tier with one shared pose through `GDT.ZONE.GROUP.PATTERN`; for a tier carrying a material-condition modifier, it shall use `GDT.ZONE.MATL.BOTH` with that tier's tolerance.
- **REQ-GDT.ZONE.COMP-006** A tier's free DOFs shall be the DRF free DOFs plus exactly the DOFs the row releases for that tier, and the engine shall never free a DOF the row does not name.
- **REQ-GDT.ZONE.COMP-007** For a multi-segment frame, each segment shall be solved as an independent tier with its own DRF, and the tier results shall carry `frame_kind: multi_segment`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/zone/comp/four_holes_composite.json` (four holes, upper tier 0.5 mm to A|B|C, lower tier 0.1 mm to A; synthetic row: lower tier releases tx, ty, rz; planted pattern shift 0.12 mm and hole-to-hole errors), when `solve_composite` runs, then tier 0 usage and tier 1 usage match the fixture within 1e-9 m and tier 1 held DOFs = {tz, rx, ry}.
- **AC-2** Given a multi-segment version with the same tolerances, then each tier equals a standalone solve with its own DRF and `frame_kind: multi_segment`.
- **AC-3** Given a lower tier with MMC, then the tier uses `GDT.ZONE.MATL.BOTH` and reports the bonus separately.
- **AC-4** (error path) Given a synthetic row with no constraint set for tier 1, then `UNSUPPORTED` and no tier values.
- **AC-5** (determinism) AC-1 is bit-identical on all CI platforms.
- **AC-6** When ORS-GDT composite cases are published, every case passes within its stated tolerance.

## Design

- **Registry command(s):** none (internal kernel API; called by `GDT.LOC.POS` and `GDT.PROF.SURF.COMP`).
- **Data structures:** `CompositeFrame { kind: Composite | MultiSegment, tiers: Vec<Tier{index, tolerance_m, datums, modifier, members}> }`; `TierConstraint { index, releases: DofSet }` read from the row's tier list; `TierSolution { index, held: DofSet, free: DofSet, pose, usage_m, members: Vec<MemberUsage>, cert, matl: Option<MatlResult> }`.
- **Algorithm:** resolve tier constraints; for each tier in order compute free DOFs; choose the solver (single, pattern, material condition); solve; package.
- **Errors:** `UNSUPPORTED`, `zone_solve_failed{code}`, `group_incomplete`.
- **Determinism:** D0 for certified tier solves.
- **Performance budget:** two tiers × 8 features × 2 000 points < 1 s [unverified].
- **Crate:** `fid-gps` (module `zone::comp`). Algorithm ID `gps.zone.composite@1.0.0`.

## Test fixtures

Analytic composite hole patterns with planted pattern shifts and feature errors (CC0, in-repo); synthetic rows with tier constraint sets; ORS-GDT composite cases read as a published artifact.

## Build prompt

> In crate `fid-gps` (kernel, KNP-1), add module `zone::comp`. Implement `solve_composite(frame: &CompositeFrame, drfs, resolution) -> Result<Vec<TierSolution>, CompError>`: read each tier's `releases` DOF set from the resolved row (missing for any tier → `UNSUPPORTED` for the whole frame); per tier, free = DRF free DOFs ∪ releases (never more); solve with `zone::pose::solve_zone` for one feature, `zone::group::pattern::solve_group` for several, and `zone::matl::solve_with_material_condition` when the tier has MMC/LMC, using that tier's tolerance; record index, held DOFs (all six minus free), pose, usage, per-member usages, certificate. Multi-segment frames: each segment independent with its own DRF and `frame_kind: multi_segment`. Algorithm ID `gps.zone.composite@1.0.0`. Tests per the acceptance criteria. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, `fid-num`, fixed-order reductions, no std `HashMap`/`HashSet`, NaN is an error); no tier semantics hard-coded.

## Open questions

- The tier constraint list (`releases` per tier index) must be expressible in a DMT row; `GDT.RES.SCHEMA` carries operator parameters, so it can live there as a canonical parameter of the chain's evaluation operator. Confirm with the RES maintainers and the DMT authors (D-009).

## Children

_Leaf: none._
