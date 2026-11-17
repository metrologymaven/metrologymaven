---
id: GDT.FORM.FLAT
title: Flatness
level: 3
parent: GDT.FORM
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.FORM-001, REQ-GDT.FORM-005]
covers: [GOM.GDT.01, PW.GDT.02]
depends_on: [GDT.FORM.FCORE, GDT.CHAR.SPEC]
side_effect: W2
determinism: D0
cli: [fid check eval-flatness]
mcp: [check_eval_flatness]
journal: [check.eval_flatness@1]
updated: 2026-11-16
---

# GDT.FORM.FLAT: Flatness

## Purpose

Evaluates flatness of a planar surface from measured points: the width of the narrowest pair of parallel planes containing the points under minimum zone, or the least-squares-referenced residual range where the row or KRN availability says so. It is the first rung of the P1a scope ladder.

## Scope

**In:** `check.eval_flatness@1`; plane element through `GDT.FORM.FCORE`; optional filtration and resampling only if the chain names them (KRN filters).
**Out:** the core policy and payload (owned by `GDT.FORM.FCORE`); filters (owned by `KRN`); flatness of a derived median plane at MMC (not in P1a; would be added with `GDT.ZONE.MATL`).

## Requirements

- **REQ-GDT.FORM.FLAT-001** The command shall evaluate flatness from a point set through the form core with element `Plane` and report value, reference plane and peak and valley indices.
- **REQ-GDT.FORM.FLAT-002** If the resolved chain names a filtration or resampling operator, then the command shall apply it with the chain's parameters before association and record its algorithm ID; otherwise it shall apply none.
- **REQ-GDT.FORM.FLAT-003** Minimum-zone flatness shall match the expected widths of the synthetic fixtures within 1e-12 m and, when run, the TraCIM Chebyshev plane test within its stated MPE [unverified].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/form/flat_known_mz.json` (seed `0xf1a7`, 500 points with planted extremes giving width 0.008 mm), when `fid check eval-flatness --char <id> --points <handle> --json` runs, then value = 0.008 mm within 1e-12 m and the peak and valley indices are the planted points.
- **AC-2** (error path) Given 3 points, then the command fails with `too_few_points` and journals nothing.
- **AC-3** (error path) Given a characteristic with no governing spec, then `governing_spec_missing` and nothing journaled.
- **AC-4** Given a KRN stub with MZ unavailable, then the result carries `conservative_upper_bound`.
- **AC-5** (replay) Exact replay of AC-1 gives `IDENTICAL`.
- **AC-6** (optional, external) The TraCIM Chebyshev plane data set passes within its MPE [unverified: access and licence of the data].

## Design

- **Registry command(s):** `check.eval_flatness@1`, W2, dry-run R1. Params: `char: CharId`, `points: PointSetRef`. Result: ResultRecord with `FormPayload`.
- **Data structures:** none beyond `FormPayload`.
- **Algorithm:** gate → resolve → optional filter/resample → `evaluate_form(Plane, …)` → builder.
- **Errors:** pass-through from the gate, resolver and form core.
- **Determinism:** D0.
- **Performance budget:** 1 M points < 500 ms including MZ [unverified].
- **Crate:** `fid-gps`.

## Test fixtures

Seeded synthetic planes with planted extremes (CC0, in-repo); TraCIM data used only through the PTB service and never committed [unverified].

## Build prompt

> In crate `fid-gps` (kernel), implement `check.eval_flatness@1` (W2, dry-run R1; CLI `fid check eval-flatness`; MCP `check_eval_flatness`; one journal event). Pipeline: governing-spec gate, `gdt.resolve_chain@1`, optional filtration/resampling only if the chain names it (record its algorithm ID), then `form::evaluate_form(FormElement::Plane, points, chain, krn)`, then the ResultRecord builder from `fid-gdt` with `evaluation_basis: POINTS`, DMT row ID and digests. Tests per the acceptance criteria (seeded synthetic fixture with planted extremes; error paths journal nothing; exact replay IDENTICAL). Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`; all input hostile) and KNP-1 (no FMA, transcendentals only through `fid-num`, fixed-order reductions, no std `HashMap`/`HashSet`, total-order min/max, NaN is an error).

## Open questions

- None.

## Children

_Leaf: none._
