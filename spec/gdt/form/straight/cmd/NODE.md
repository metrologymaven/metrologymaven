---
id: GDT.FORM.STRAIGHT.CMD
title: Straightness command and dispatch
level: 4
parent: GDT.FORM.STRAIGHT
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.FORM.STRAIGHT-001, REQ-GDT.FORM.STRAIGHT-002]
covers: [GOM.GDT.02, PW.GDT.02]
depends_on: [GDT.FORM.STRAIGHT.LINEEL, GDT.FORM.STRAIGHT.DERIVED, GDT.CHAR.SPEC, GDT.CHAR.RECORD, GDT.RES.RESOLVE]
side_effect: W2
determinism: D3
cli: [fid check eval-straightness]
mcp: [check_eval_straightness]
journal: [check.eval_straightness@1]
updated: 2026-11-17
---

# GDT.FORM.STRAIGHT.CMD: Straightness command and dispatch

## Purpose

Owns `check.eval_straightness@1`: gate the governing spec, resolve the chain, and dispatch to line-element or derived-median-line evaluation by what the resolved chain contains (a reduction to a median line means the derived path; otherwise the line-element path), then build the ResultRecord. Dispatching on the chain, not on FCF symbols, keeps the interpretation in the DMT.

## Scope

**In:** `check.eval_straightness@1` (W2; dry-run R1); dispatch rule on the chain; ResultRecord assembly with `evaluation_basis: POINTS`, the payload of the chosen path and the path name (`line_elements` or `derived_median_line`).
**Out:** the evaluations (owned by `GDT.FORM.STRAIGHT.LINEEL`, `GDT.FORM.STRAIGHT.DERIVED`); the dispatch criterion's content (owned by DMT rows).

## Requirements

- **REQ-GDT.FORM.STRAIGHT.CMD-001** When the resolved chain contains a reduction operator producing a median line, the command shall evaluate the derived path; otherwise it shall evaluate the line-element path; the payload shall name the path.
- **REQ-GDT.FORM.STRAIGHT.CMD-002** The command shall pass the governing-spec gate and resolver first, and any error shall journal nothing and emit no ResultRecord.
- **REQ-GDT.FORM.STRAIGHT.CMD-003** The ResultRecord shall carry `evaluation_basis: POINTS`, the DMT row ID and digests, and every flag of the chosen path.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/form/straight_lines_planted.json` and a synthetic row without a median-line reduction, when `fid check eval-straightness --char C --points P --json` runs, then the payload names `line_elements` and value = 0.006 mm within 1e-12 m.
- **AC-2** Given a synthetic row with a median-line reduction and a derivation stub, then the payload names `derived_median_line`.
- **AC-3** (error path) Given no governing spec, then `governing_spec_missing` and nothing journaled.
- **AC-4** (replay) Exact replay of AC-1 gives `IDENTICAL`.

## Design

- **Registry command(s):** `check.eval_straightness@1`, W2, dry-run R1; params `char: CharId`, `points: PointSetRef`; result ResultRecord with `StraightnessPayload { path, line_elements? , derived? }`.
- **Data structures:** `StraightnessPayload`.
- **Algorithm:** gate, resolve, inspect chain operators, dispatch, builder.
- **Errors:** pass-through from gate, resolver and the two paths.
- **Determinism:** D3 on the derived path (iterative fits), D0 on the line-element path; recorded per result.
- **Performance budget:** dominated by the chosen path [unverified].
- **Crate:** `fid-gps` (module `form::straightness`).

## Test fixtures

Fixtures of `GDT.FORM.STRAIGHT.LINEEL` and `GDT.FORM.STRAIGHT.DERIVED`; synthetic DMT rows with placeholder content (CC0).

## Build prompt

> In crate `fid-gps`, module `form::straightness`, implement registry command `check.eval_straightness@1` (W2, dry-run R1, `--json`; CLI `fid check eval-straightness`; MCP `check_eval_straightness`; one journal event). Pipeline: governing-spec gate, resolve chain, then if the chain contains a reduction operator producing a median line call `evaluate_derived_straightness`, else `evaluate_line_elements`; build the ResultRecord with `evaluation_basis: POINTS`, DMT identifiers, the path name and its payload and flags. Errors journal nothing. Tests per acceptance criteria. Constraints: CONSTITUTION §8, KNP-1, no std `HashMap`/`HashSet`; GDT never decides conformance.

## Open questions

- None.

## Children

_Leaf: none._
