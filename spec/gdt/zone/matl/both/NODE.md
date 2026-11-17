---
id: GDT.ZONE.MATL.BOTH
title: Axis and surface variants with the deciding variant
level: 4
parent: GDT.ZONE.MATL
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.ZONE.MATL-001, REQ-GDT.ZONE.MATL-002, REQ-GDT.ZONE.MATL-004]
covers: [GOM.GDT.15]
depends_on: [GDT.ZONE.MATL.BONUS, GDT.ZONE.MATL.VCB, GDT.ZONE.POSE.SOLVE, GDT.RES.RESOLVE]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.ZONE.MATL.BOTH: Axis and surface variants with the deciding variant

## Purpose

The public `solve_with_material_condition` entry point. It runs the variant the resolved row selects (axis interpretation: derived axis in a zone enlarged by the bonus; surface interpretation: virtual-condition boundary), and, when the characteristic or defaults profile asks to report both, runs the other variant too, marks which one the row says decides, and flags disagreement. Callers such as `GDT.LOC.POS` get one result type whatever the variant.

## Scope

**In:** variant dispatch from the resolution's variant name; axis variant (zone of size stated + bonus around the derived axis or median points the caller supplies, via `solve_zone`); surface variant (via `GDT.ZONE.MATL.VCB`); report-both mode; `deciding_variant`; `interpretations_disagree` flag.
**Out:** derived axis extraction (owned by `GDT.LOC.DERIV`); bonus arithmetic (owned by `GDT.ZONE.MATL.BONUS`); boundary solve (owned by `GDT.ZONE.MATL.VCB`); which variant decides (DMT content).

## Requirements

- **REQ-GDT.ZONE.MATL.BOTH-001** The module shall run the axis variant when the resolved variant name is `axis` and the surface variant when it is `surface`, and fail with `unknown_variant` for any other name.
- **REQ-GDT.ZONE.MATL.BOTH-002** The axis variant shall return the zone usage of the derived feature from `solve_zone`, the `BonusRecord`, and `axis_ok_margin` = total tolerance − usage in metres.
- **REQ-GDT.ZONE.MATL.BOTH-003** When report-both is requested, the module shall run both variants, mark the resolved variant as `deciding_variant`, and set `interpretations_disagree` when one margin is negative and the other non-negative.
- **REQ-GDT.ZONE.MATL.BOTH-004** Flags `local_optimum_only` from either variant shall be carried on that variant's part of the result.
- **REQ-GDT.ZONE.MATL.BOTH-005** Results shall reproduce every ORS-GDT position/MMC case within its stated tolerance, including cases where the two interpretations disagree.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/zone/matl/disagree.json` (hole with form error such that the axis variant passes with margin 0.005 mm and the surface variant fails with clearance −0.003 mm), when run in report-both mode with resolved variant `surface`, then both margins match within 1e-9 m, `deciding_variant` = surface and `interpretations_disagree` is set.
- **AC-2** Given a perfect-form hole fixture, then both variants agree and no flag is set.
- **AC-3** (error path) Given variant name `envelope`, then `unknown_variant`.
- **AC-4** (determinism) AC-1 is bit-identical on all CI platforms.

## Design

- **Registry command(s):** none (internal kernel API).
- **Data structures:** `MatlResult { deciding_variant, axis: Option<AxisPart{usage, bonus, margin, cert}>, surface: Option<SurfacePart{VcResult}>, flags }`.
- **Algorithm:** dispatch by variant name; compute margins; compare signs.
- **Errors:** `unknown_variant`, pass-through.
- **Determinism:** D0.
- **Performance budget:** the two solves plus constant overhead.
- **Crate:** `fid-gps` (module `zone::matl`).

## Test fixtures

Analytic hole fixtures with planted form error chosen so the interpretations disagree (CC0, in-repo).

## Build prompt

> In crate `fid-gps` (kernel, KNP-1), module `zone::matl`, implement `solve_with_material_condition(input, resolution, report_both) -> Result<MatlResult, MatlError>`. Variant `axis`: `bonus(...)`, then `solve_zone` on the caller-supplied derived axis or median points with the zone shape the caller names; margin = total − usage. Variant `surface`: `vc_containment(...)`; margin = minimum clearance. Any other variant name → `unknown_variant`. In report-both mode run both, mark the resolved one `deciding_variant`, set `interpretations_disagree` when the margins have opposite signs (negative versus non-negative). Carry `local_optimum_only` per part. Tests per the acceptance criteria. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, NaN is an error); never hard-code which variant decides.

## Open questions

- None beyond the parent's precedence question.

## Children

_Leaf: none._
