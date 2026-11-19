---
id: OUTR.BENCH.RUN.RECIPE
title: Benchmark recipe format and validator
level: 4
parent: OUTR.BENCH.RUN
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-outreach]
built_pr: 174
satisfies: [REQ-OUTR.BENCH.RUN-001]
covers: []
depends_on: []
uses_artifacts: [ECO.DATA.CAT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.BENCH.RUN.RECIPE: Benchmark recipe format and validator

## Purpose

The one file format every public benchmark is written in, and the validator that refuses a recipe missing anything needed to reproduce it. ACC, FIDEL, PERF and CMP add a kind-specific section to the same file; this node owns the common part and the hook through which kind sections are validated.

## Scope

**In:** the `bench-recipe@1` TOML schema (common fields, source types, result fields and tolerances); the `bench-recipe check` validator; the hook that hands a kind section to its kind validator; fixtures.
**Out:** kind-specific sections and their rules (`OUTR.BENCH.ACC.FMT`, `OUTR.BENCH.FIDEL`, `OUTR.BENCH.PERF`, `OUTR.BENCH.CMP`); running recipes (`OUTR.BENCH.RUN.HARNESS`); pages (`OUTR.BENCH.RUN.PAGES`); the dataset catalogue (`ECO.DATA.CAT`).

## Requirements

- **REQ-OUTR.BENCH.RUN.RECIPE-001** A recipe shall state `schema`, `id`, `kind`, `title`, `since_release`, `dataset` (catalogue ID and SHA-256), `machine_profile`, a `source`, and at least one result field with name, unit, direction and regression tolerance, and the validator shall fail naming every missing or malformed field.
- **REQ-OUTR.BENCH.RUN.RECIPE-002** If a recipe's dataset ID is not in the ECO.DATA.CAT catalogue, or its SHA-256 differs from the catalogue entry, then the validator shall fail with `dataset_unknown` or `dataset_digest_mismatch`.
- **REQ-OUTR.BENCH.RUN.RECIPE-003** A recipe source shall be either `release_numbers` (a JSON Pointer into the release-numbers bundle plus a pointer to the run's journal) or `command` (an argument vector for the released `fid` or `fid-verify` binary, run with journal recording), and the validator shall reject any other source type and any argument vector that names a different program.
- **REQ-OUTR.BENCH.RUN.RECIPE-004** Each result field's tolerance shall be a decimal string with a kind of `abs` or `rel`, and its direction shall be `lower`, `higher` or `equal` (which value is better).
- **REQ-OUTR.BENCH.RUN.RECIPE-005** If a recipe's `kind` is `accuracy`, `fidelity`, `performance` or `comparison`, then the validator shall require the matching kind section and pass it to the registered kind validator; any other kind shall fail with `kind_unknown`.
- **REQ-OUTR.BENCH.RUN.RECIPE-006** Recipe IDs shall be unique across the recipe folder and match `^[a-z][a-z0-9-]*(\.[a-z0-9-]+)+$`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/bench/recipe/ok/` (one recipe of each kind with stub kind validators, and a catalogue file listing their datasets), when `fid-outreach bench-recipe check --catalogue catalogue.toml recipes/ --json` runs, then three kinds are validated (accuracy, fidelity and performance are listed as valid with `kind_check` `stub`) and comparison is flagged with `kind_blocked` citing D-021, so it exits 1.
- **AC-2** (error path) Given `missing-tolerance/` (a field without `tolerance`), when the check runs, then it exits 1 with `field_malformed` naming the recipe and field.
- **AC-3** (error path) Given `bad-digest/` (dataset SHA-256 differs from the catalogue), when the check runs, then it exits 1 with `dataset_digest_mismatch`.
- **AC-4** (error path) Given `other-program/` (`source.argv[0] = "python"`), when the check runs, then it exits 1 with `source_invalid`.
- **AC-5** (error path) Given `dup-id/` (two files with the same `id`), when the check runs, then it exits 1 with `duplicate_id` naming both files.
- **AC-6** (error path) Given an unparsable TOML file, when the check runs, then it exits 2 and names the file.
- **AC-7** (determinism) Given the same folder, when the check runs twice with the files listed in different orders, then the JSON reports are byte-identical.

## Design

- **Command:** `fid-outreach bench-recipe check --catalogue <file> [--allowlist <file>] [--json] <paths>...` (`--allowlist` defaults to the nearest `house-rules/spdx-allowlist.toml` above the catalogue). Project tooling, not a registry command; nearest class R0.
- **Schema `bench-recipe@1`:**
  - `schema = "bench-recipe@1"`, `id`, `kind`, `title`, `since_release` (release tag), `machine_profile` (`minimum`, `recommended` or `any`; `any` only for deterministic results);
  - `dataset = { id, sha256 }`;
  - `[source]` either `type = "release_numbers"`, `selector` (JSON Pointer), `journal_selector` (JSON Pointer to the journal reference), or `type = "command"`, `argv` (first element `fid` or `fid-verify`), `outputs` (JSON Pointer per field);
  - `[[fields]]` `name`, `unit` (catalogue unit symbol or `1` for counts and ratios), `better`, `tolerance`, `tolerance_kind`;
  - one kind section `[accuracy]`, `[fidelity]`, `[performance]` or `[comparison]`.
- **Kind hook:** a trait `KindValidator { fn kind(&self) -> &str; fn validate(&self, section, recipe) -> Vec<Finding> }`; validators are registered at build time in a `BTreeMap` keyed by kind.
- **Errors:** `field_missing`, `field_malformed`, `dataset_unknown`, `dataset_digest_mismatch`, `source_invalid`, `kind_unknown`, `kind_section_missing`, `duplicate_id`, `id_malformed`, `dataset_licence` (the dataset's catalogue entry states no `licence`, or one not in `asset_licences` of the ECO.RULES.SPDX allowlist), `kind_blocked` (a kind that must fail closed: `comparison` until OUTR.BENCH.CMP, per D-021); exit 0 ok, 1 findings, 2 unreadable or unparsable input or no recipe file found. Each valid recipe is reported with `kind_check` = `full` (checked by a real kind validator) or `stub` (placeholder only).
- **Determinism:** D0; files read in sorted path order, BTreeMap only, RFC 8785 JSON output.
- **Crate:** `fid-outreach`, module `bench::recipe`.

## Test fixtures

Hand-written TOML recipes and a stub catalogue under `fixtures/outr/bench/recipe/` (`ok`, `missing-tolerance`, `bad-digest`, `other-program`, `dup-id`, `unparsable`), CC-BY-4.0. Dataset digests are of small synthetic files in the fixture folder.

## Build prompt

> In the Rust crate `fid-outreach` (outreach tooling, not part of the product), add module `bench::recipe` and subcommand `fid-outreach bench-recipe check --catalogue <file> [--json] <paths>...`.
>
> A benchmark recipe is a TOML file with `schema = "bench-recipe@1"`, `id` (pattern `^[a-z][a-z0-9-]*(\.[a-z0-9-]+)+$`, unique across inputs), `kind` (`accuracy`, `fidelity`, `performance`, `comparison`), `title`, `since_release`, `machine_profile` (`minimum`, `recommended`, `any`), `dataset = { id, sha256 }`, a `[source]` table, one or more `[[fields]]` (`name`, `unit`, `better` = lower|higher|equal, `tolerance` decimal string, `tolerance_kind` = abs|rel), and exactly one kind section named after `kind`.
>
> Source is either `type = "release_numbers"` with `selector` and `journal_selector` (JSON Pointers into the release-numbers bundle) or `type = "command"` with `argv` whose first element is `fid` or `fid-verify` and `outputs` (one JSON Pointer per field). Reject anything else with `source_invalid`.
>
> Check the dataset against the dataset catalogue file given by `--catalogue` (TOML entries with `id` and `sha256`): `dataset_unknown` or `dataset_digest_mismatch`. Define a `KindValidator` trait and a registry keyed by kind; this PR ships stub validators that accept any section, and other PRs register real ones. Missing kind section: `kind_section_missing`; unknown kind: `kind_unknown`.
>
> Output findings with file, recipe ID, field and error code, sorted; RFC 8785 canonical JSON with `--json`. Exit 0 when valid, 1 on findings, 2 on unreadable or unparsable input.
>
> Tests with fixtures under `fixtures/outr/bench/recipe/`: `ok` (exit 0, four recipes), `missing-tolerance` (`field_malformed`), `bad-digest` (`dataset_digest_mismatch`), `other-program` (`source_invalid`), `dup-id` (`duplicate_id`), `unparsable` (exit 2), and byte-identical output regardless of argument order.
>
> Constraints: no panics, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded reads (refuse files over 1 MB); no network; BTreeMap rather than HashMap; dependencies only under allowlisted licences.

## Open questions

- The unit symbols accepted for `[[fields]] unit` are a compiled-in list in `fid-outreach` (`bench::recipe::UNITS`). It is temporary: replace it with the GEO.FRAME.UNITS catalogue once that exists.
- The Design rule "`machine_profile = "any"` only for deterministic results" is not enforced by this validator: the common part has no field saying whether a result is deterministic. The kind validators (PERF in particular) must enforce it.
- The catalogue file path and schema are owned by ECO.DATA.CAT; this leaf reads only `id`, `sha256` and `licence` per entry.

## Children

_Leaf: none._
