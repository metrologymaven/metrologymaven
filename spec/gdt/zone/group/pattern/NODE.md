---
id: GDT.ZONE.GROUP.PATTERN
title: Shared-pose solve for a pattern
level: 4
parent: GDT.ZONE.GROUP
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.ZONE.GROUP-001, REQ-GDT.ZONE.GROUP-004]
covers: []
depends_on: [GDT.ZONE.GROUP.IDS, GDT.ZONE.POSE.LIN, GDT.ZONE.POSE.SOLVE, GDT.ZONE.ZTYPE]
uses_artifacts: [UNC.ORS.GDT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.ZONE.GROUP.PATTERN: Shared-pose solve for a pattern

## Purpose

Solves a pattern of features (for example four holes under one position callout) with one pose of the zone framework shared by every member, within the DRF's free DOFs. Each member keeps its own zone shape and allowed size (members at MMC can have different totals after bonus), so the problem minimises the largest usage-to-allowance ratio over members at a common pose, then reports each member's usage at that pose. It is the P1a building block that simultaneous requirements later reuse.

## Scope

**In:** `solve_group(members, drf, free, objective)` with members `{char_id, shape, nominal_location_in_framework, points, allowance_m}`; one set of pose variables; per-member rows from the LIN layout scaled by 1/allowance; objective `max_ratio` (default) or the row's alternative; per-member usage and ratio at the certified pose; `GroupRef` via IDS.
**Out:** group identity and the all-or-nothing rule (owned by `GDT.ZONE.GROUP.IDS`); bonus (owned by `GDT.ZONE.MATL`); cross-callout grouping (owned by `GDT.ZONE.GROUP.SIMREQ`); position payloads (owned by `GDT.LOC.POS`).

## Requirements

- **REQ-GDT.ZONE.GROUP.PATTERN-001** The solver shall use one set of pose variables (the DRF's free DOFs) for all members, with each member's zone placed at its nominal location in the pattern framework.
- **REQ-GDT.ZONE.GROUP.PATTERN-002** With objective `max_ratio`, the solver shall minimise max_k(usage_k / allowance_k) at the shared pose, and report for each member its usage and ratio recomputed with `GDT.ZONE.ZTYPE` at the certified pose.
- **REQ-GDT.ZONE.GROUP.PATTERN-003** When the free set is empty, the solver shall evaluate each member at the DRF pose without a solver.
- **REQ-GDT.ZONE.GROUP.PATTERN-004** If any member has no points or an allowance ≤ 0, then the solver shall return `group_incomplete` through `GDT.ZONE.GROUP.IDS` and no member values.
- **REQ-GDT.ZONE.GROUP.PATTERN-005** The result shall carry the `GroupRef` (group ID and shared pose digest), the certificate or `local_optimum_only`, and the driving member index.
- **REQ-GDT.ZONE.GROUP.PATTERN-006** The solver shall reproduce every ORS-GDT pattern case within its stated tolerance.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/zone/group/four_holes.json` (four hole axes on a 50 mm square, framework free in {tx, ty, rz} relative to datum A, planted common shift 0.03 mm plus individual offsets), when solved, then the recovered framework shift matches the planted common shift within 1e-9 m and each member's usage matches the fixture within 1e-9 m.
- **AC-2** Given the same pattern with a full DRF, then no solver is called and usages equal per-member fixed-pose values.
- **AC-3** Given members with allowances 0.10 and 0.14 mm (MMC bonus), then the reported ratios are usage/allowance and the driving member has the largest ratio.
- **AC-4** (error path) Given a member with no points, then `group_incomplete{member}` and no values.
- **AC-5** (determinism) AC-1 is bit-identical on all CI platforms; the group ID and pose digest match IDS.
- **AC-6** When ORS-GDT pattern cases are published, every case passes within its stated tolerance.

## Design

- **Registry command(s):** none (internal kernel API).
- **Data structures:** `GroupMember { char_id, shape, location, points, allowance_m }`; `GroupSolution { group: GroupRef, pose, members: Vec<MemberUsage{char_id, usage_m, ratio, driving_point}>, driving_member, cert }`.
- **Algorithm:** stack each member's LIN rows (positions transformed into the framework) with a shared ratio variable ρ: member rows bounded by ρ·allowance_k/2; minimise ρ with the KRN sequential solver; certify; recompute per member with ZTYPE.
- **Errors:** `group_incomplete`, `zone_solve_failed{code}`.
- **Determinism:** D0 for certified results.
- **Performance budget:** 16 members × 2 000 points, three DOFs < 500 ms [unverified].
- **Crate:** `fid-gps` (module `zone::group::pattern`).

## Test fixtures

Analytic hole and pin patterns with planted framework shifts (CC0, in-repo); ORS-GDT pattern cases read as a published artifact.

## Build prompt

> In crate `fid-gps` (kernel, KNP-1), add module `zone::group::pattern`. Implement `solve_group(members: &[GroupMember], drf, free: DofSet, objective) -> Result<GroupSolution, GroupError>`: one set of pose variables shared by all members; each member's zone at its nominal location in the framework, its LIN rows bounded by ρ·allowance/2; minimise ρ (objective `max_ratio`) with the sequential solver from `fid-solv`; certify; recompute each member's usage and ratio with `ZoneShape::usage` at the certified pose; driving member = largest ratio (lowest index on ties); no solver when the free set is empty; any member without points or with allowance ≤ 0 → `group_incomplete` via `zone::group::all_or_nothing`. Attach `GroupRef` (group ID, pose digest) and the certificate or `local_optimum_only`. Tests per the acceptance criteria. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, `fid-num`, fixed-order reductions, total-order min/max, no std `HashMap`/`HashSet`, NaN is an error).

## Open questions

- Minimising the largest usage-to-allowance ratio is the natural choice when allowances differ [unverified against the standards]; a row may name another objective, and the DMT authors should confirm the default.

## Children

_Leaf: none._
