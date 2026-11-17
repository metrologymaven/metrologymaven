---
id: GDT.PROF.BESTFIT.OPTS
title: Best-fit option model, DOF selection and flags
level: 4
parent: GDT.PROF.BESTFIT
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.PROF.BESTFIT-001, REQ-GDT.PROF.BESTFIT-002, REQ-GDT.PROF.BESTFIT-003]
covers: [PCD.DIM.06]
depends_on: [GDT.DRF.BUILD, GDT.RES.RESOLVE]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.PROF.BESTFIT.OPTS: Best-fit option model, DOF selection and flags

## Purpose

Decides, for a profile check, which pose DOFs the zone solve may move and with which objective, and produces the record that says so. It is pure bookkeeping over 6-bit DOF sets: the drawing's baseline (the DRF's free DOFs, or all six with no datums), the user's option, the row's restrictions, and the `bestfit_beyond_drf` flag when an option frees what the DRF constrains. Keeping it separate means every profile command flags non-drawing best fits the same way.

## Scope

**In:** `BestFitOption { None, Free, Constrained{dofs}, Mobility }`; `Objective { MinimumZone, LeastSquares }`; `pose_dofs`; row restrictions via limitation codes; `BestFitRecord`.
**Out:** the solves themselves (owned by `GDT.ZONE.POSE`, `GDT.ZONE.MOBSRCH`, `GDT.PROF.BESTFIT.LSQ`); which options a callout permits (DMT content in `GDT.RES.ROWASME` and `GDT.RES.ROWISO`).

## Requirements

- **REQ-GDT.PROF.BESTFIT.OPTS-001** For option `None`, the free DOFs shall equal the baseline (the DRF's free DOFs, or all six when the characteristic has no datums).
- **REQ-GDT.PROF.BESTFIT.OPTS-002** For option `Free`, the free DOFs shall be all six; for `Constrained{dofs}`, exactly the named DOFs; for `Mobility`, the baseline with `needs_mobility = true`.
- **REQ-GDT.PROF.BESTFIT.OPTS-003** The module shall compute `beyond_drf` as the free DOFs that the DRF constrains, and when it is non-empty the record shall carry flag `bestfit_beyond_drf` and list those DOFs in the order tx, ty, tz, rx, ry, rz.
- **REQ-GDT.PROF.BESTFIT.OPTS-004** If the resolved row carries limitation code `bestfit_disallowed:<option>` for the selected option, then the module shall return `bestfit_option_not_allowed` naming the option.
- **REQ-GDT.PROF.BESTFIT.OPTS-005** The objective shall be `MinimumZone` unless the resolved chain's parameter `bestfit_objective` is `least_squares`, and the record shall name it.
- **REQ-GDT.PROF.BESTFIT.OPTS-006** If `Mobility` is selected and no datum feature is referenced at MMB or LMB, then the module shall return `no_datum_mobility`.
- **REQ-GDT.PROF.BESTFIT.OPTS-007** When no option is given, the module shall use the default option named by the resolved chain (parameter `bestfit_default`), or `None` if the chain names none.
- **REQ-GDT.PROF.BESTFIT.OPTS-008** `BestFitRecord { option, source, objective, free, beyond_drf, needs_mobility, transform }` shall be filled with the transform the caller reports after solving, and `source` shall be `user`, `chain_default` or `none_default`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/prof/bestfit/dof_table.json` (DRFs A, A|B, A|B|C and none; all four options; constrained sets {tz}, {rx, ry}), when `pose_dofs` runs, then each result's free set, `beyond_drf` and `needs_mobility` match the table exactly.
- **AC-2** Given DRF A|B|C and option `Free`, then `beyond_drf` = all six and the flag is set; given option `None`, then the free set is empty and no flag.
- **AC-3** (error path) Given a synthetic row with `bestfit_disallowed:free` and option `Free`, then `bestfit_option_not_allowed{free}`.
- **AC-4** (error path) Given option `Mobility` with all datums at RMB, then `no_datum_mobility`.
- **AC-5** Given no option and a chain with `bestfit_default: constrained{tz}`, then source = `chain_default` and free = {tz}.

## Design

- **Registry command(s):** none (library used by `check.eval_surface_profile@1` and `check.eval_line_profile@1`).
- **Data structures:** `DofSet` (6-bit, from `GDT.DRF.BUILD`); `BestFitOption`, `Objective`, `PoseDofs { free, beyond_drf, needs_mobility, objective }`, `BestFitRecord` as in REQ-008.
- **Algorithm:** set arithmetic on 6-bit masks; limitation-code lookup by exact string match in sorted order.
- **Errors:** `bestfit_option_not_allowed{option}`, `no_datum_mobility`, `bad_dof_name`.
- **Determinism:** D0 (no floating point).
- **Performance budget:** trivial.
- **Crate:** `fid-gps` (module `prof::bestfit`).

## Test fixtures

`fixtures/gdt/prof/bestfit/dof_table.json` (hand-written truth table, CC0); synthetic DMT rows only.

## Build prompt

> In crate `fid-gps` (kernel, pure), add module `prof::bestfit`. Define `BestFitOption { None, Free, Constrained{dofs: DofSet}, Mobility }`, `Objective { MinimumZone, LeastSquares }`, `PoseDofs`, `BestFitRecord { option, source, objective, free, beyond_drf, needs_mobility, transform }`. Implement `pose_dofs(option: Option<BestFitOption>, drf: Option<&Drf>, chain: &OperatorChain, limitations: &[LimitationCode]) -> Result<PoseDofs, BestFitError>`: baseline = DRF free DOFs or all six without datums; `None` → baseline; `Free` → all six; `Constrained{d}` → exactly d; `Mobility` → baseline with `needs_mobility` (error `no_datum_mobility` if no datum is at MMB/LMB); `beyond_drf` = free ∩ DRF-constrained, flagged `bestfit_beyond_drf`; refuse with `bestfit_option_not_allowed{option}` when the row carries `bestfit_disallowed:<option>`; default option from chain parameter `bestfit_default`, else `None`; objective from chain parameter `bestfit_objective` (default minimum zone). Tests per the acceptance criteria (truth table, flags, error paths). Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), no std `HashMap`/`HashSet`; no interpretation hard-coded (which options a callout allows comes from the row).

## Open questions

- The limitation-code convention `bestfit_disallowed:<option>` must be accepted by the DMT authors and by `GDT.RES.LINT` (limitation codes are free codes in the schema today).

## Children

_Leaf: none._
