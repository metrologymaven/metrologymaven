---
id: GDT.FORM.CYL
title: Cylindricity
level: 3
parent: GDT.FORM
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.FORM-001, REQ-GDT.FORM-005]
covers: [GOM.GDT.04, PW.GDT.02]
depends_on: [GDT.FORM.FCORE, GDT.CHAR.SPEC]
side_effect: W2
determinism: D0
cli: [fid check eval-cylindricity]
mcp: [check_eval_cylindricity]
journal: [check.eval_cylindricity@1]
updated: 2026-11-16
---

# GDT.FORM.CYL: Cylindricity

## Purpose

Evaluates cylindricity from points on a cylindrical surface: the radial width of the narrowest pair of coaxial cylinders containing the points (minimum zone), or the least-squares-referenced radial range where the row or KRN availability says so. The minimum-zone cylinder is non-convex, so the `local_optimum_only` flag matters here more than for flatness.

## Scope

**In:** `check.eval_cylindricity@1`; cylinder element through `GDT.FORM.FCORE`; optional filtration if the chain names it.
**Out:** form policy (owned by `GDT.FORM.FCORE`); roundness (owned by `GDT.FORM.ROUND`); straightness of the derived median line (owned by `GDT.FORM.STRAIGHT`).

## Requirements

- **REQ-GDT.FORM.CYL-001** The command shall evaluate cylindricity through the form core with element `Cylinder` and report value, reference axis and radius, and peak and valley indices.
- **REQ-GDT.FORM.CYL-002** If the KRN minimum-zone cylinder fit reports `local_optimum_only` or multi-start disagreement, then the result shall carry `local_optimum_only`.
- **REQ-GDT.FORM.CYL-003** Minimum-zone cylindricity shall match the synthetic fixtures within 1e-12 m and, when run, the TraCIM Chebyshev cylinder test within its stated MPE [unverified].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/form/cyl_known_mz.json` (seed `0xc71d`, 2 000 points on a cylinder r = 10 mm with planted radial extremes giving width 0.005 mm), when `fid check eval-cylindricity --json` runs, then value = 0.005 mm within 1e-12 m.
- **AC-2** Given a KRN stub returning `local_optimum_only`, then the result carries the flag.
- **AC-3** (error path) Given 4 points with a row minimum of 5, then `too_few_points` and nothing journaled.
- **AC-4** (replay) Exact replay of AC-1 gives `IDENTICAL`.
- **AC-5** (optional, external) TraCIM Chebyshev cylinder data passes within its MPE [unverified].

## Design

- **Registry command(s):** `check.eval_cylindricity@1`, W2, dry-run R1. Params: `char`, `points`. Result: ResultRecord with `FormPayload`.
- **Data structures:** none new.
- **Algorithm:** gate → resolve → optional filter → `evaluate_form(Cylinder, …)` → builder.
- **Errors:** pass-through.
- **Determinism:** D0 (KRN multi-start is deterministic).
- **Performance budget:** 100 000 points < 1 s including MZ [unverified].
- **Crate:** `fid-gps`.

## Test fixtures

Seeded synthetic cylinders with planted extremes (CC0, in-repo); TraCIM through the service only.

## Build prompt

> In crate `fid-gps` (kernel), implement `check.eval_cylindricity@1` (W2, dry-run R1; CLI `fid check eval-cylindricity`; MCP `check_eval_cylindricity`; one journal event). Pipeline: governing-spec gate, resolve chain, optional filter if named, `form::evaluate_form(FormElement::Cylinder, …)`, ResultRecord builder with `evaluation_basis: POINTS`. Propagate `local_optimum_only` from the KRN minimum-zone cylinder fit. Tests per the acceptance criteria. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`; all input hostile) and KNP-1 (no FMA, transcendentals only through `fid-num`, fixed-order reductions, no std `HashMap`/`HashSet`, total-order min/max, NaN is an error).

## Open questions

- None.

## Children

_Leaf: none._
