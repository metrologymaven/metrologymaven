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

- **Partial build (not marked `built_in`).** Open ACs: **AC-2** (cone diameter at a gauge: the cone fit `KRN.LSQ.AXIS.CONE`, PR #420, is not on main) and **AC-5** as written (see the flag rule below). AC-1, AC-3, AC-4 and AC-6 are met for the circle (`circle2`, `circle3`), cylinder and sphere. Built: `fid_gps::dim::radial` (pure: feature fit against the chain's single association step, the least-squares fits of `fid-fit` through the `Fit` trait, `2 r` or `r`) and `fid_gdt::dim::radial` (`check.eval_diameter@1`, `check.eval_radius@1`, W2 / dry run R1, CLI and MCP entry points, registry definitions, the ResultRecord through `GDT.CHAR.RECORD`'s builder with the D-045 interpretation block, the journal event and exact replay against the pinned register). Reference: `fixtures/gdt/dim/make_radial.py` (planted rational geometry, stdlib only). When #420 merges: add `fit.cone.ls@1.0.0` to the available fits, compute `r(g) = g tan(half_angle)` through `fid-num`, and test AC-2 against `diameter_at_gauge` in `radial_analytic.json`; `from_plane(PlaneRef, m)` waits for the same change.
- **Dependencies not yet marked built.** `KRN.LSQ.RND` (#417) and `KRN.LSQ.AXIS.CYL` are used although they are not `built_in`: their open ACs are reference data (D-038) and their command layer (CORE.REG), not the fits this leaf calls (maintainer decision, 2026-11-21).
- **Flags are refusals, not record fields (REQ-004, AC-5).** Maintainer rule: a fit flagged `local_optimum_only` or `suspect_axis`, ill-conditioned or refused is never the basis of a value. The cylinder runs through `Fit::fit`, which refuses a flagged fit (`FlaggedFit`), and the command fails with `fit_refused{flagged_fit}` carrying the flags; no record is made. So no record ever carries `local_optimum_only`. Proposal: reword REQ-004 / AC-5 to "fails with `fit_refused{flagged_fit}` naming the flag".
- **Feature input.** There is no feature model yet, so `FeatureRef` is a measured feature: its points plus the fit algorithm it is defined with (`RadialFeatures`, content-addressed), and the command fits it. A fitted, nominal or claimed radius cannot be passed in, so nothing claimed looks measured (D-045); the record's basis is `POINTS`. `fit_method_mismatch` compares the feature's fit with the chain's association step exactly (algorithm ID and version); a chain with two association steps is `ambiguous_association`.
- **Chain.** The chain is resolved for DMT characteristic types `diameter` and `radius` (no modifier), with an optional variant override per call. The DMT has no such rows yet; the tests use the synthetic rows of `radial_table.json`. Expert rows are D-009 / D-016 content.
- **Record fields.** The record's `algorithm` is the fit's ID (REQ-003); the leaf's payload `{value_si, fit_alg, flags}` has no slot in `fid.gdt-result/2`, so the fit digest and chain go to the command result and the event. Uncertainty is UEC-0 (`u_missing`) and no decision rule is bound (`NOT_DECIDABLE`, `no_rule_bound`) until UNC wiring exists. The authored-unit conversion of REQ-001 / REQ-GDT.DIM-002 is not done here: the record holds `si:m`, and the characteristic model that would carry the authored unit does not exist.
- **Journal entry.** A record names the journal entry holding its event, which does not exist before the event is committed. So the command returns the value and the event, and `commit` finishes the record with the entry's hash. The event is not authority-bearing (no approval, sign-off or decision), so REQ-CORE.RPL.RECON.FOLD-008 does not apply; replay still takes the principal and entry hash from the entry, never the event body.
- **D-045 pacing.** Diameter and radius are plain dimensions, not callouts of the first-review subset. This build does not claim or mark the leaf built; whether it counts as shared machinery is for `GDT.RES.PACE`.

## Children

_Leaf: none._
