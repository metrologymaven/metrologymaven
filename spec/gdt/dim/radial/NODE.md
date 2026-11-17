---
id: GDT.DIM.RADIAL
title: Diameter and radius checks
level: 3
parent: GDT.DIM
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.DIM-001, REQ-GDT.DIM-002, REQ-GDT.DIM-007]
covers: [GOM.DIM.04, PW.GDT.01]
depends_on: [GDT.CHAR.SPEC, GDT.CHAR.RECORD, GDT.RES.RESOLVE, KRN.LSQ]
side_effect: W2
determinism: D0
cli: [fid check eval-diameter, fid check eval-radius]
mcp: [check_eval_diameter, check_eval_radius]
journal: [check.eval_diameter@1, check.eval_radius@1]
updated: 2026-11-16
---

# GDT.DIM.RADIAL: Diameter and radius checks

## Purpose

Reports the diameter or radius of a fitted circle, cylinder or sphere, and of a cone at a named gauge position, as a plain dimension. The fit method is the one the resolved operator chain names (for example least squares, minimum circumscribed or maximum inscribed from KRN). Feature-of-size evaluation under ASME Rule #1 is separate (`GDT.DIM.SIZEASME`).

## Scope

**In:** diameter and radius of circle, cylinder, sphere; cone diameter at a gauge distance along the axis from the apex or from a named reference plane; recording which fit method produced the feature; ResultRecord.
**Out:** fitting (owned by `KRN`); two-point local size and envelope (owned by `GDT.DIM.SIZEASME`); ISO 14405-1 size modifiers (owned by `GDT.DIM.SIZEISO`).

## Requirements

- **REQ-GDT.DIM.RADIAL-001** The system shall report diameter as twice the fitted radius of the referenced circle, cylinder or sphere, and radius as the fitted radius, in SI internally and the authored unit externally.
- **REQ-GDT.DIM.RADIAL-002** For a cone, the system shall report the diameter at the stated gauge distance and fail with `gauge_position_missing` if none is stated.
- **REQ-GDT.DIM.RADIAL-003** The ResultRecord shall record the algorithm ID of the fit that produced the feature, and if that fit differs from the association named in the resolved chain, the command shall fail with `fit_method_mismatch`.
- **REQ-GDT.DIM.RADIAL-004** If the referenced fit carries `local_optimum_only`, then the ResultRecord shall carry the same flag.
- **REQ-GDT.DIM.RADIAL-005** Values on the analytic fixtures shall match within 1e-12 m [unverified].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/dim/radial_analytic.json` (LS-fitted circle r = 5 mm, cylinder r = 12.5 mm, sphere r = 10 mm, cone with half-angle 30 degrees), when `fid check eval-diameter --json` and `fid check eval-radius --json` run, then the values match within 1e-12 m.
- **AC-2** Given the cone with gauge distance 10 mm from apex, then the diameter is 2·10·tan(30°) mm within 1e-12 m.
- **AC-3** (error path) Given a cone without gauge position, then the command fails with `gauge_position_missing` and journals nothing.
- **AC-4** (error path) Given a feature fitted by LS while the chain names maximum inscribed, then the command fails with `fit_method_mismatch`.
- **AC-5** Given a fit flagged `local_optimum_only`, then the record carries the flag.
- **AC-6** (replay) Exact replay of AC-1 gives `IDENTICAL`.

## Design

- **Registry command(s):** `check.eval_diameter@1`, `check.eval_radius@1`; W2, dry-run R1. Params: `char`, `feature: FeatureRef`, `gauge: Option<GaugePosition>`. Result: ResultRecord with payload `{value_si, fit_alg, flags}`.
- **Data structures:** `GaugePosition { from_apex(m) | from_plane(PlaneRef, m) }`.
- **Algorithm:** gate → resolve → check fit method matches chain → read radius (cone: r(z) = z·tan(half_angle) via `fid-num`) → builder.
- **Errors:** `gauge_position_missing`, `fit_method_mismatch`, `not_radial_feature`, pass-through gate and resolver errors.
- **Determinism:** D0.
- **Performance budget:** < 5 µs per evaluation excluding the fit [unverified].
- **Crate:** `fid-gps`.

## Test fixtures

Synthetic fitted features (analytic, no noise) generated in-repo; CC0.

## Build prompt

> In crate `fid-gps` (kernel), implement `check.eval_diameter@1` and `check.eval_radius@1` (W2, dry-run R1; CLI `fid check eval-diameter`, `fid check eval-radius`; MCP `check_eval_diameter`, `check_eval_radius`; one journal event per committed command). Gate on the governing spec, resolve the chain, refuse with `fit_method_mismatch` if the referenced feature's fit algorithm differs from the chain's association, then report 2r or r for circle, cylinder, sphere, and cone diameter at a required gauge position (`gauge_position_missing`). Carry `local_optimum_only` from the fit. Build the ResultRecord with `fid-gdt`'s builder, recording the fit's algorithm ID. Tests: acceptance criteria above. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`; all input hostile) and KNP-1 (no FMA, transcendentals only through `fid-num`, fixed-order reductions, no std `HashMap`/`HashSet`, total-order min/max, NaN is an error).

## Open questions

- None.

## Children

_Leaf: none._
