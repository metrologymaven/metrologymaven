---
id: OUTR.BENCH.ACC.EVAL
title: GD&T evaluation accuracy recipes
level: 4
parent: OUTR.BENCH.ACC
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-OUTR.BENCH.ACC-005]
covers: []
depends_on: [OUTR.BENCH.ACC.FMT]
uses_artifacts: [UNC.ORS, ECO.DATA.CAT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.BENCH.ACC.EVAL: GD&T evaluation accuracy recipes

## Purpose

Public accuracy recipes for dimensional and GD&T evaluations, built on the released outcome reference suites (UNC.ORS): each recipe runs one suite with the released product and reports deviations from the suite's reference results. Like ACC.FIT, these are data files plus a test.

## Scope

**In:** recipe files under `bench/recipes/accuracy/eval/`, one per released ORS suite that is public; the test that every recipe validates and every public suite has a recipe.
**Out:** the accuracy section rules and table (`OUTR.BENCH.ACC.FMT`); fit recipes (`OUTR.BENCH.ACC.FIT`); the suites and `rdl.run_suite@1` (`UNC.ORS`).

## Requirements

- **REQ-OUTR.BENCH.ACC.EVAL-001** There shall be one recipe per released public ORS suite, with source `command` running `fid rdl run-suite --json` on that suite.
- **REQ-OUTR.BENCH.ACC.EVAL-002** Each recipe shall name the suite release by ID and SHA-256 as its dataset, and that suite release shall be in the ECO.DATA.CAT catalogue.
- **REQ-OUTR.BENCH.ACC.EVAL-003** Each recipe shall state the dialect and standard edition the suite evaluates (for example ASME Y14.5-2018) in the accuracy section's reference fields, by designator and edition only.
- **REQ-OUTR.BENCH.ACC.EVAL-004** A repository test shall fail if a released public suite has no recipe or if any recipe fails the validator.

## Acceptance criteria

- **AC-1** Given `bench/recipes/accuracy/eval/` and the catalogue, when `fid-outreach bench-recipe check --catalogue <catalogue> bench/recipes/accuracy/eval/` runs, then it exits 0.
- **AC-2** Given the suite listing (`fid rdl list-suite --json`, committed as a fixture), when the coverage test runs, then every public suite has exactly one recipe.
- **AC-3** (error path) Given fixture `fixtures/outr/bench/acc/eval/missing/` (a listing with a suite that has no recipe), when the coverage test runs, then it fails naming the suite.

## Design

- **Registry command(s):** none; data files and a test. `source.argv` is `["fid", "rdl", "run-suite", "--suite", "<suite id>", "--json"]` [argument names to be matched to UNC.ORS's generated CLI].
- **Fields:** per characteristic output, `computed`, `reference`, `deviation`, `uec`, plus `decision_match` (count of conformance decisions equal to the reference decision, unit `1`, better `higher`, tolerance `0` abs).
- **Determinism:** D0 for files and test; suite outcome as declared by UNC.ORS.
- **Crate:** recipe data in the outreach repository; test in `fid-outreach` (`tests/acc_eval.rs`).

## Test fixtures

A committed suite listing fixture and `fixtures/outr/bench/acc/eval/missing/`, hand-written, CC-BY-4.0; the suites themselves come from UNC.ORS releases.

## Build prompt

> In the outreach repository, add accuracy recipes under `bench/recipes/accuracy/eval/`, one `bench-recipe@1` file per released public outcome reference suite: `kind = "accuracy"`, ID `acc.eval.<suite-id>`, dataset = the suite release ID and SHA-256 from the dataset catalogue, `machine_profile = "any"`, source `command` running the released `fid rdl run-suite --json` on that suite. In `[accuracy]`, set the reference fields to the suite name, version and ID, and name the standard and edition the suite evaluates by designator and edition only. Fields: per output computed, reference, deviation, UEC, plus `decision_match` (tolerance 0).
>
> Add `tests/acc_eval.rs` in `fid-outreach` that runs the recipe validator over the folder and checks against a committed suite-listing JSON that every public suite has exactly one recipe (fixture `fixtures/outr/bench/acc/eval/missing/` must fail naming the suite).
>
> Constraints: no network in tests; no standards text anywhere; no reference values written into recipes.

## Open questions

- Which ORS suites are public (as opposed to blind) is recorded by UNC.ORS; this leaf follows that flag.

## Children

_Leaf: none._
