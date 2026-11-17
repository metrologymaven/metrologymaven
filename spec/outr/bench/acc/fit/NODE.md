---
id: OUTR.BENCH.ACC.FIT
title: Feature-fit accuracy recipes
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
uses_artifacts: [UNC.RDL.FREEZE, UNC.RDL.AST, ECO.DATA.CAT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.BENCH.ACC.FIT: Feature-fit accuracy recipes

## Purpose

The set of public accuracy recipes for feature fitting (least-squares and minimum-zone fits of lines, planes, circles, spheres and cylinders, and the other fit methods as they land), each running the released self-test over a frozen reference-data release and reporting deviations from the reference values. These are data files plus a test, not new code.

## Scope

**In:** recipe files under `bench/recipes/accuracy/fit/`; the choice of RDL cases per recipe; the test that every recipe validates.
**Out:** the accuracy section rules and table (`OUTR.BENCH.ACC.FMT`); GD&T evaluation recipes (`OUTR.BENCH.ACC.EVAL`); the reference data and the self-test command (`UNC.RDL`).

## Requirements

- **REQ-OUTR.BENCH.ACC.FIT-001** There shall be one recipe per fit method and geometry pair that the frozen RDL release covers, each with source `command` running `fid rdl run-selftest --json` restricted to that pair's cases.
- **REQ-OUTR.BENCH.ACC.FIT-002** Each recipe shall name the frozen RDL release set by ID and SHA-256 as its dataset, and that set shall be in the ECO.DATA.CAT catalogue.
- **REQ-OUTR.BENCH.ACC.FIT-003** Each recipe shall use only RDL cases whose recorded licence allows the use stated in its `licence_use`, and cases from blind (tier B) sets shall never be used.
- **REQ-OUTR.BENCH.ACC.FIT-004** A repository test shall run the recipe validator over the folder and fail on any finding.

## Acceptance criteria

- **AC-1** Given the recipe folder `bench/recipes/accuracy/fit/` and the catalogue, when `fid-outreach bench-recipe check --catalogue <catalogue> bench/recipes/accuracy/fit/` runs, then it exits 0.
- **AC-2** Given the frozen RDL release listing (`fid rdl list-dataset --json` over the release), when the coverage test runs, then every fit method and geometry pair in the listing has exactly one recipe.
- **AC-3** (error path) Given fixture `fixtures/outr/bench/acc/fit/blind.toml` (a recipe selecting a tier B case), when the coverage test runs, then it fails with `blind_case_used`.

## Design

- **Registry command(s):** none; data files and a test. Each recipe's `source.argv` is `["fid", "rdl", "run-selftest", "--release", "<set id>", "--case-prefix", "<pair>", "--json"]` [argument names to be matched to UNC.RDL.AST's generated CLI].
- **Fields:** per output, `computed`, `reference`, `deviation`, `uec`; tolerance per field `0` abs for `reference` (it must not change between releases) and a per-method abs tolerance for `deviation` written in the recipe [values unverified until the first run].
- **Coverage test:** reads the listing, groups cases by method and geometry, compares with recipe IDs `acc.fit.<method>-<geometry>`.
- **Determinism:** D0 for the files and the test; the self-test outcome is D1 per UNC.RDL.AST.
- **Crate:** recipe data in the outreach repository; test in `fid-outreach` (`tests/acc_fit.rs`).

## Test fixtures

The frozen RDL release set published by UNC.RDL.FREEZE (licences per case recorded there), and the hand-written `fixtures/outr/bench/acc/fit/blind.toml`, CC-BY-4.0.

## Build prompt

> In the outreach repository, add accuracy recipes under `bench/recipes/accuracy/fit/`, one `bench-recipe@1` file per fit method and geometry pair present in the frozen reference-data (RDL) release: `kind = "accuracy"`, ID `acc.fit.<method>-<geometry>`, dataset = the frozen release set ID and SHA-256 from the dataset catalogue, `machine_profile = "any"`, source `command` running the released `fid rdl run-selftest` with `--json`, restricted to that pair's cases. Fill the `[accuracy]` section (reference source name, edition, dataset ID, licence and `licence_use` copied from the catalogue; outputs with computed, reference, deviation and UEC field names). Never select blind (tier B) cases.
>
> Add `tests/acc_fit.rs` in `fid-outreach` that (1) runs the recipe validator over the folder and fails on any finding and (2) reads the RDL listing JSON and fails if a method and geometry pair has no recipe or two, or if a recipe uses a tier B case (`blind_case_used`, fixture `fixtures/outr/bench/acc/fit/blind.toml`).
>
> Constraints: no panics or `unwrap` in test helpers either; no network in tests (use the listing JSON committed as a fixture); write no reference values into recipes (they come from the run).

## Open questions

- The exact CLI argument names of `fid rdl run-selftest` come from the generated CLI of UNC.RDL.AST; the recipes use them once it ships.

## Children

_Leaf: none._
