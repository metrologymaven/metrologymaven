---
id: GDT.DIM.SCALAR
title: Scalar property and min/max checks
level: 3
parent: GDT.DIM
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.DIM-001, REQ-GDT.DIM-002, REQ-GDT.DIM-007]
covers: [GOM.DIM.06, GOM.DIM.08]
depends_on: [GDT.CHAR.SPEC, GDT.CHAR.RECORD, GDT.RES.RESOLVE, KRN.LSQ]
side_effect: W2
determinism: D0
cli: [fid check eval-scalar, fid check eval-minmax]
mcp: [check_eval_scalar, check_eval_minmax]
journal: [check.eval_scalar@1, check.eval_minmax@1]
updated: 2026-11-16
---

# GDT.DIM.SCALAR: Scalar property and min/max checks

## Purpose

Checks a single numeric property of an element (for example a fitted feature's length, cone half-angle, slot width or a constructed point's coordinate) against its tolerance, and checks the minimum, maximum or range over a set of values or deviations. These are generic checks that let users tolerance anything the model exposes as a number.

## Scope

**In:** a typed, closed **property registry** listing each element property, its quantity kind (length, angle, dimensionless) and source; scalar check on one property; min, max and range over a set of values (properties of several elements, or a deviation set from `GDT.DIM.PTDEV` or SCAN); ResultRecord.
**Out:** section-derived values (owned by `GDT.DIM.SECT`); computing deviations (owned by `GDT.DIM.PTDEV` and `SCAN`); fitting (owned by `KRN`).

## Requirements

- **REQ-GDT.DIM.SCALAR-001** The scalar check shall accept only properties listed in the property registry and shall fail with `unknown_property` otherwise.
- **REQ-GDT.DIM.SCALAR-002** The scalar check shall report the property value with the registry's quantity kind and refuse a characteristic whose tolerance unit has a different quantity kind (`quantity_kind_mismatch`).
- **REQ-GDT.DIM.SCALAR-003** The min/max check shall report min, max and range over the set and the index of the element or point that attains each, choosing the lowest index on ties.
- **REQ-GDT.DIM.SCALAR-004** If the value set is empty or contains a NaN, then the command shall fail with `empty_value_set` or `nan_value` and emit no ResultRecord.
- **REQ-GDT.DIM.SCALAR-005** Values on the analytic fixtures shall match within 1e-12 of the value's SI unit [unverified].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/dim/scalar_props.json` (a cylinder of length 40 mm, a cone of half-angle 15 degrees), when `fid check eval-scalar --property cylinder.length --json` and `--property cone.half_angle` run, then values match within 1e-12 m and 1e-12 rad.
- **AC-2** (error path) Given `--property cylinder.colour`, then the command fails with `unknown_property` and journals nothing.
- **AC-3** (error path) Given a length tolerance on `cone.half_angle`, then it fails with `quantity_kind_mismatch`.
- **AC-4** Given fixture `minmax_set.json` (1 000 seeded deviations with two equal maxima), when `fid check eval-minmax --json` runs, then min, max and range match the fixture and the max index is the lower of the two.
- **AC-5** (error path) Given an empty set, then `empty_value_set`.
- **AC-6** (replay) Exact replay of AC-1 and AC-4 gives `IDENTICAL`.

## Design

- **Registry command(s):** `check.eval_scalar@1` and `check.eval_minmax@1`; W2, dry-run R1. Params: `char`, `element` and `property` (scalar), or `values: ValueSetRef` and `statistic: min | max | range` (min/max). Result: ResultRecord with payload `{value_si, attained_at}`.
- **Data structures:** `PropertyRegistry`: a static sorted table `(element_kind, property) -> (quantity_kind, accessor)`; adding a property is a reviewed code change.
- **Algorithm:** gate → resolve → registry lookup → read value, or one fixed-order pass with total-order min/max over the set → builder.
- **Errors:** `unknown_property`, `quantity_kind_mismatch`, `empty_value_set`, `nan_value`, pass-through gate and resolver errors.
- **Determinism:** D0.
- **Performance budget:** min/max over 10 M values < 50 ms [unverified].
- **Crate:** `fid-gps`.

## Test fixtures

Synthetic features and a seeded value set generated in-repo; CC0.

## Build prompt

> In crate `fid-gps` (kernel), implement a static property registry `(element_kind, property) -> (quantity_kind, accessor)` and the commands `check.eval_scalar@1` and `check.eval_minmax@1` (W2, dry-run R1; CLI `fid check eval-scalar`, `fid check eval-minmax`; MCP `check_eval_scalar`, `check_eval_minmax`; one journal event each). Gate on the governing spec and resolve the chain first. Scalar: reject unknown properties (`unknown_property`) and quantity-kind mismatches (`quantity_kind_mismatch`). Min/max: one fixed-order pass with total-order min/max, report the attaining index (lowest on ties), fail on empty sets (`empty_value_set`) and NaN (`nan_value`). Build the ResultRecord with `fid-gdt`'s builder. Tests: acceptance criteria above. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`; all input hostile) and KNP-1 (no FMA, transcendentals only through `fid-num`, fixed-order reductions, no std `HashMap`/`HashSet`, total-order min/max, NaN is an error).

## Open questions

- None.

## Children

_Leaf: none._
