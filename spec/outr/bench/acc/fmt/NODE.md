---
id: OUTR.BENCH.ACC.FMT
title: Accuracy recipe section and result table
level: 4
parent: OUTR.BENCH.ACC
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-OUTR.BENCH.ACC-001, REQ-OUTR.BENCH.ACC-002, REQ-OUTR.BENCH.ACC-003, REQ-OUTR.BENCH.ACC-004]
covers: []
depends_on: [OUTR.BENCH.RUN.RECIPE]
uses_artifacts: [ECO.DATA.CAT, UNC.RDL.FORMAT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.BENCH.ACC.FMT: Accuracy recipe section and result table

## Purpose

The `[accuracy]` section of a benchmark recipe, its validator, and the result-table fragment that every accuracy page shows: computed value, reference value, deviation with units, the reference source cited by name, edition and dataset ID, and the UEC level of each value. It makes the reporting rules for accuracy claims a check rather than a habit.

## Scope

**In:** the `[accuracy]` schema; the kind validator registered under `accuracy`; the fragment writer that renders the accuracy table into the result store; the licence check against the dataset catalogue; the citation length limit.
**Out:** the common recipe format (`OUTR.BENCH.RUN.RECIPE`); which datasets are used (`OUTR.BENCH.ACC.FIT`, `OUTR.BENCH.ACC.EVAL`); reference data records (`UNC.RDL.FORMAT`); computing any value (the released product).

## Requirements

- **REQ-OUTR.BENCH.ACC.FMT-001** The accuracy section shall state `reference_source` (name), `reference_edition` (edition or version), `reference_dataset` (identifier at the source), `licence` (SPDX ID or the catalogue's licence string), `licence_use` (`reference` or `redistribute`) and one `[[accuracy.outputs]]` entry per compared output with its name, unit and the result field that holds its UEC level.
- **REQ-OUTR.BENCH.ACC.FMT-002** If the section's `licence` or `licence_use` disagrees with the dataset's ECO.DATA.CAT entry (licence string or redistribution status), then the validator shall fail with `licence_mismatch`.
- **REQ-OUTR.BENCH.ACC.FMT-003** If any citation field is longer than 200 characters or the section contains a field not in the schema, then the validator shall fail, so that no excerpt of a standard or reference document can be carried in a recipe.
- **REQ-OUTR.BENCH.ACC.FMT-004** For each output, the fragment shall show the computed value, the reference value and the deviation (computed minus reference, or the magnitude when the source reports only a magnitude, labelled as such), each with its unit.
- **REQ-OUTR.BENCH.ACC.FMT-005** For each output, the fragment shall show the UEC level, and when the level is UEC-0 or absent it shall print `not evaluated` and never print an uncertainty value.
- **REQ-OUTR.BENCH.ACC.FMT-006** The fragment shall cite the reference source as name, edition and dataset identifier on one line, with no other text from the source.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/bench/acc/ok.toml` and catalogue `catalogue.toml` (dataset licensed CC0-1.0, mirrored), when `fid-outreach bench-recipe check --catalogue catalogue.toml ok.toml` runs with the accuracy validator registered, then it exits 0.
- **AC-2** (error path) Given `licence-mismatch.toml` (`licence_use = "redistribute"` for a fetch-only catalogue entry), when the check runs, then it exits 1 with `licence_mismatch`.
- **AC-3** (error path) Given `long-citation.toml` (a `reference_source` of 250 characters), when the check runs, then it exits 1 with `citation_too_long`.
- **AC-4** Given `result-uec0.json` (an output with UEC level 0) and the recipe from AC-1, when `fid-outreach bench-acc fragment --recipe ok.toml --result result-uec0.json` runs, then the fragment row for that output reads `not evaluated` in the UEC column and contains no uncertainty number.
- **AC-5** Given `result-magnitude.json` (the source reports only |d|), when the fragment is written, then the deviation column header reads `|deviation|`.
- **AC-6** (determinism) Given the same inputs, when the fragment is written twice, then the files are byte-identical.

## Design

- **Command:** `fid-outreach bench-acc fragment --recipe <file> --result <file> [--out <file>]`, also called by the harness through the fragment hook; validation runs inside `bench-recipe check`. Project tooling; nearest class R0.
- **Schema `[accuracy]`:** `reference_source`, `reference_edition`, `reference_dataset`, `licence`, `licence_use`, `deviation = "signed" | "magnitude"`, `[[accuracy.outputs]] { name, unit, computed_field, reference_field, deviation_field, uec_field }`; unknown keys rejected.
- **Fragment:** a Markdown table `output | computed | reference | deviation | unit | UEC` followed by one citation line `Reference: <name>, <edition>, dataset <id>`.
- **Errors:** `licence_mismatch`, `citation_too_long`, `unknown_key`, `output_field_missing`.
- **Determinism:** D0; values are copied as decimal strings, never re-rounded.
- **Crate:** `fid-outreach`, module `bench::accuracy`.

## Test fixtures

Hand-written recipes, result records and a catalogue under `fixtures/outr/bench/acc/` (`ok`, `licence-mismatch`, `long-citation`, `result-uec0`, `result-magnitude`), CC-BY-4.0. Reference names in fixtures are fictional.

## Build prompt

> In the Rust crate `fid-outreach`, add module `bench::accuracy`: a `KindValidator` for kind `accuracy` (registered with the existing `bench::recipe` registry) and a fragment writer `fid-outreach bench-acc fragment --recipe <file> --result <file> [--out <file>]`, also exposed to `bench::harness` through its fragment hook.
>
> The `[accuracy]` recipe section has `reference_source`, `reference_edition`, `reference_dataset`, `licence`, `licence_use` (`reference` or `redistribute`), `deviation` (`signed` or `magnitude`) and `[[accuracy.outputs]]` with `name`, `unit`, `computed_field`, `reference_field`, `deviation_field`, `uec_field`. Reject unknown keys (`unknown_key`) and any citation field over 200 characters (`citation_too_long`). Compare `licence` and `licence_use` with the dataset's catalogue entry (licence string; `redistribute` needs the entry to allow mirroring): `licence_mismatch`.
>
> The fragment is a Markdown table with columns output, computed, reference, deviation (header `|deviation|` when `deviation = "magnitude"`), unit and UEC, then one line `Reference: <name>, <edition>, dataset <id>`. When the UEC level is 0 or missing, print `not evaluated` and no uncertainty value. Copy numbers as the decimal strings in the result record; never round or recompute them.
>
> Tests with fixtures under `fixtures/outr/bench/acc/`: valid recipe (exit 0), licence mismatch, long citation, UEC-0 row shows `not evaluated`, magnitude header, byte-identical fragments.
>
> Constraints: no panics, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; BTreeMap rather than HashMap; no network; allowlisted dependency licences only. Never include text from standards or reference documents in code, fixtures or messages.

## Open questions

- The UNC.RDL.AST run record exposes `abs_d_plus_ud` per output, not a signed deviation and UEC level; until UNC publishes those fields, accuracy pages use `deviation = "magnitude"` and UEC-0. Cross-domain issue for UNC.RDL.AST.

## Children

_Leaf: none._
