---
id: GDT.ZONE.MATL.BONUS
title: Bonus tolerance from actual size
level: 4
parent: GDT.ZONE.MATL
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.ZONE.MATL-001, REQ-GDT.ZONE.MATL-003, REQ-GDT.ZONE.MATL-004]
covers: [GOM.GDT.15, PCD.DIM.04]
depends_on: [GDT.DIM.SIZEASME, GDT.RES.RESOLVE]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
built_in: [fid-gps]
updated: 2026-11-23
---

# GDT.ZONE.MATL.BONUS: Bonus tolerance from actual size

## Purpose

The arithmetic of a material-condition modifier on a toleranced feature of size: given the stated geometric tolerance, the MMC or LMC size from the size tolerance, the feature's kind (internal or external) and the actual size the resolved row names (supplied by the size evaluation), compute the departure from the material condition and the bonus, and report stated tolerance, bonus and total tolerance as separate fields. Which actual size counts is the row's choice; this leaf only does the arithmetic and carries flags.

## Scope

**In:** signed departure δ from the material-condition size (positive away from that condition); bonus = max(δ, 0); total = stated + bonus; optional cap from the row (`bonus_cap: opposite_limit`); flags; `local_optimum_only` pass-through; `BonusRecord`.
**Out:** actual size (owned by `GDT.DIM.SIZEASME`, `GDT.DIM.SIZEISO`); using the total tolerance in a zone (owned by `GDT.LOC.POS` and other callers via `GDT.ZONE.POSE`); boundary checks (owned by `GDT.ZONE.MATL.VCB`).

## Requirements

- **REQ-GDT.ZONE.MATL.BONUS-001** The module shall compute δ as (actual − MMC size) for an internal feature at MMC, (MMC size − actual) for an external feature at MMC, (LMC size − actual) for an internal feature at LMC, and (actual − LMC size) for an external feature at LMC.
- **REQ-GDT.ZONE.MATL.BONUS-002** The bonus shall be max(δ, 0), and when δ < 0 the record shall carry `actual_beyond_material_limit`.
- **REQ-GDT.ZONE.MATL.BONUS-003** When the chain sets `bonus_cap: opposite_limit`, the bonus shall not exceed |MMC size − LMC size|, and the record shall carry `bonus_capped` when the cap applies.
- **REQ-GDT.ZONE.MATL.BONUS-004** The record shall report `stated_m`, `bonus_m`, `total_m`, the actual-size definition ID from the row, and the actual size used, as separate fields.
- **REQ-GDT.ZONE.MATL.BONUS-005** If the actual size carries `local_optimum_only`, then the record shall carry the same flag.
- **REQ-GDT.ZONE.MATL.BONUS-006** If any input is NaN, the stated tolerance is negative, or the row names no actual-size definition, then the module shall return `bad_material_input` or `actual_size_undefined`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/zone/matl/bonus_table.json` (hole Ø10.000–10.100 mm at MMC, stated 0.050 mm, actual sizes 10.000, 10.040, 10.100, 9.990 mm; pin variants; LMC variants), when computed, then bonus and total match the table within 1e-15 m and the 9.990 mm hole carries `actual_beyond_material_limit` with bonus 0.
- **AC-2** Given `bonus_cap: opposite_limit` and actual 10.150 mm, then bonus = 0.100 mm and `bonus_capped`.
- **AC-3** Given an actual size flagged `local_optimum_only`, then the record carries it.
- **AC-4** (error path) Given NaN actual, then `bad_material_input`; given a row without an actual-size definition, then `actual_size_undefined`.

## Design

- **Registry command(s):** none (internal kernel API).
- **Data structures:** `MaterialCondition { Mmc, Lmc }`; `FeatureKind { Internal, External }`; `BonusRecord { stated_m, bonus_m, total_m, actual_m, actual_def, flags }`.
- **Algorithm:** one subtraction, a max and an optional min; total = stated + bonus.
- **Errors:** `bad_material_input`, `actual_size_undefined`.
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-gps` (module `zone::matl`).

## Test fixtures

Hand-written truth table (CC0, in-repo); synthetic rows only.

## Build prompt

> In crate `fid-gps` (kernel, pure, KNP-1), add module `zone::matl` with `bonus(stated, mmc_size, lmc_size, kind: Internal|External, cond: Mmc|Lmc, actual: ActualSize, chain) -> Result<BonusRecord, MatlError>`. δ = actual − MMC (internal, MMC), MMC − actual (external, MMC), LMC − actual (internal, LMC), actual − LMC (external, LMC); bonus = max(δ, 0) with flag `actual_beyond_material_limit` when δ < 0; optional cap |MMC − LMC| when chain `bonus_cap: opposite_limit` (flag `bonus_capped`); total = stated + bonus; carry `local_optimum_only` from the actual size; record stated, bonus, total, actual and the row's actual-size definition ID separately. Errors `bad_material_input`, `actual_size_undefined`. Tests per the acceptance criteria. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, NaN is an error).

## Open questions

- Whether a feature whose actual size violates its material limit gets zero bonus or is refused is DMT content [unverified]; this leaf reports zero with a flag, and the size characteristic reports the violation separately.

## Children

_Leaf: none._
