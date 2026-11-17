---
id: GDT.DIM.SIZEASME.CMD
title: Size command, dialect dispatch and ASME size payload
level: 4
parent: GDT.DIM.SIZEASME
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.DIM.SIZEASME-001, REQ-GDT.DIM.SIZEASME-002, REQ-GDT.DIM.SIZEASME-003]
covers: [PW.GDT.01]
depends_on: [GDT.DIM.SIZEASME.LOCAL, GDT.DIM.SIZEASME.ENVLP, GDT.CHAR.SPEC, GDT.CHAR.RECORD, GDT.RES.RESOLVE]
side_effect: W2
determinism: D3
cli: [fid check eval-size]
mcp: [check_eval_size]
journal: [check.eval_size@1]
updated: 2026-11-17
---

# GDT.DIM.SIZEASME.CMD: Size command, dialect dispatch and ASME size payload

## Purpose

Owns `check.eval_size@1`: gate the governing spec, resolve the chain, dispatch by dialect to a registered size evaluator (the ASME evaluator here; the ISO evaluator is registered by `GDT.DIM.SIZEISO.DISP`), and assemble the size payload and ResultRecord. The ASME evaluator reports minimum and maximum two-point local size and the Rule #1 envelope result together, and exposes the actual size that `GDT.ZONE.MATL` reads for bonus.

## Scope

**In:** `check.eval_size@1` (W2; dry-run R1); `SizeEvaluator` trait and a dialect-keyed registry populated at startup (`ASME` here, `ISO` by `GDT.DIM.SIZEISO.DISP`); the ASME evaluator calling `local_sizes` and `envelope` as the chain names (either may be absent if the row omits it); `SizePayload`; `ActualSize` accessor for `GDT.ZONE.MATL`; `evaluation_basis: POINTS`.
**Out:** pairing (owned by `GDT.DIM.SIZEASME.LOCAL`); envelope (owned by `GDT.DIM.SIZEASME.ENVLP`); ISO modifiers (owned by `GDT.DIM.SIZEISO`); bonus (owned by `GDT.ZONE.MATL`).

## Requirements

- **REQ-GDT.DIM.SIZEASME.CMD-001** When `check.eval_size@1` runs, the command shall pass the governing-spec gate, resolve the chain, and dispatch to the evaluator registered for the characteristic's dialect; if none is registered, then it shall return `UNSUPPORTED{dialect}` and journal nothing.
- **REQ-GDT.DIM.SIZEASME.CMD-002** The ASME payload shall contain minimum and maximum local size with their index pairs, envelope size, signed clearance, within-boundary boolean, envelope scope and all flags, each omitted (not zero) when the row does not call for it.
- **REQ-GDT.DIM.SIZEASME.CMD-003** If any sub-result carries `local_optimum_only`, then the ResultRecord shall carry it.
- **REQ-GDT.DIM.SIZEASME.CMD-004** The command shall expose `actual_size(result) -> Option<(f64, AlgId)>` returning the envelope size the row designates as actual size, for `GDT.ZONE.MATL`.
- **REQ-GDT.DIM.SIZEASME.CMD-005** Values shall be reported in the authored unit with the SI value retained, and errors from the gate, resolver or evaluators shall journal nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/size/hole_lobed.json`, a characteristic with ASME governing spec, a synthetic DMT row naming a test pairing operator and the MIC fit, and a `local_sizes` stub returning known sizes, when `fid check eval-size --char C --points P --json` runs, then the payload has min, max, envelope size, clearance and within-boundary as in `expected/size_hole_lobed.json`.
- **AC-2** Given an ISO characteristic and no ISO evaluator registered, then `UNSUPPORTED{dialect: ISO}` and nothing journaled.
- **AC-3** Given a row with no envelope step, then the payload has no envelope fields (keys absent).
- **AC-4** Given an envelope stub flagged `local_optimum_only`, then the ResultRecord carries the flag.
- **AC-5** Given AC-1, `actual_size` returns the envelope size and the MIC algorithm ID.
- **AC-6** (replay) Exact replay of AC-1 gives `IDENTICAL` on the same platform (D3 class recorded).

## Design

- **Registry command(s):** `check.eval_size@1`, W2, dry-run R1; params `char: CharId`, `points: PointSetRef`; result ResultRecord with `SizePayload`.
- **Data structures:** `trait SizeEvaluator { fn evaluate(&self, points, chain, limits) -> Result<SizePayload, SizeError>; }`; `SizeRegistry: BTreeMap<Dialect, Box<dyn SizeEvaluator>>`; `SizePayload` with optional fields.
- **Algorithm:** gate, resolve, dispatch, builder with `evaluation_basis: POINTS`.
- **Errors:** `UNSUPPORTED{dialect}`, pass-through from gate, resolver, LOCAL and ENVLP.
- **Determinism:** D3 (envelope fits are iterative); the record states the class.
- **Performance budget:** dominated by fits [unverified].
- **Crate:** `fid-gps` (module `dim::size`).

## Test fixtures

Fixtures of `GDT.DIM.SIZEASME.ENVLP`; a synthetic DMT table with a test pairing operator (placeholder content, no standards text, CC0).

## Build prompt

> In crate `fid-gps`, module `dim::size`, implement registry command `check.eval_size@1` (W2, dry-run R1, `--json`; CLI `fid check eval-size`; MCP `check_eval_size`). Pipeline: governing-spec gate, resolve chain, look up a `SizeEvaluator` by dialect in a `BTreeMap` registry (return `UNSUPPORTED{dialect}` if absent), evaluate, build the ResultRecord with `evaluation_basis: POINTS`. Implement the ASME evaluator: call `local_sizes(points, kind, chain)` and `envelope(points, kind, mmc, chain, krn)` only for the steps the chain contains; payload fields min/max local size with index pairs, envelope size, signed clearance, within-boundary, envelope scope, flags; omit absent fields. Expose `actual_size(result)` for the material-condition engine. Propagate `local_optimum_only`. Tests per acceptance criteria with stubs for `local_sizes`. Constraints: CONSTITUTION §8, KNP-1, no std `HashMap`/`HashSet`; GDT never decides conformance (the builder binds the UNC rule).

## Open questions

- None.

## Children

_Leaf: none._
