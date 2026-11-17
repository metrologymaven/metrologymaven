---
id: OUTR.BENCH.RUN.PAGES
title: Result-page generator with regression display
level: 4
parent: OUTR.BENCH.RUN
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.BENCH.RUN-002, REQ-OUTR.BENCH.RUN-004, REQ-OUTR.BENCH.RUN-005, REQ-OUTR.BENCH.RUN-006]
covers: []
depends_on: [OUTR.BENCH.RUN.HARNESS, OUTR.BRAND.LINT]
uses_artifacts: [PLAT.HW.REFMACH]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.BENCH.RUN.PAGES: Result-page generator with regression display

## Purpose

Turn the result store into static benchmark pages: one page per recipe with its recipe fields, run date and journal link, the current release next to the previous one for every field, and a visible regression mark when a field got worse by more than its tolerance. The generator then runs the outreach claim lint on its own output and refuses to hand pages to the website if any error remains.

## Scope

**In:** the `bench-pages` command; page layout per recipe and per kind index; stable URLs; previous-release comparison and regression marking; inclusion of kind fragments; running the claim lint on generated pages.
**Out:** the result store and records (`OUTR.BENCH.RUN.HARNESS`); kind fragments' content (`OUTR.BENCH.ACC.FMT`, `OUTR.BENCH.FIDEL`, `OUTR.BENCH.PERF`, `OUTR.BENCH.CMP`); site build and hosting (`OUTR.SITE.GEN`); the lint rules (`OUTR.BRAND.LINT`).

## Requirements

- **REQ-OUTR.BENCH.RUN.PAGES-001** Each recipe page shall show the recipe ID, title, dataset ID and digest, machine ID, profile and host, the release, the run date, the command or bundle selector used, and a download link to the stored journal with its SHA-256.
- **REQ-OUTR.BENCH.RUN.PAGES-002** For every result field, the page shall show the current release's value and the previous release's value with units, or `no previous result` when the recipe has none.
- **REQ-OUTR.BENCH.RUN.PAGES-003** If a field is worse than the previous release's value by more than its tolerance (absolute or relative, in the recipe's `better` direction, compared exactly as decimals), then the page shall mark the field `regressed` with the size of the change, and shall still show the newer value.
- **REQ-OUTR.BENCH.RUN.PAGES-004** If the machine registry records a comparability break for the machine between the two releases, then the field shall be marked `not comparable` instead of compared.
- **REQ-OUTR.BENCH.RUN.PAGES-005** A result whose status is not `ok` shall be shown with its status and reason and no number, and shall never be replaced by an older value.
- **REQ-OUTR.BENCH.RUN.PAGES-006** Each recipe shall have a stable URL path `bench/<recipe-id>/` for the latest release and `bench/<recipe-id>/<release>/` per release, and these paths shall not change between generator versions.
- **REQ-OUTR.BENCH.RUN.PAGES-007** After writing pages, the generator shall run the outreach claim lint over them, and if the lint reports any error, then the generator shall exit non-zero and leave no output for the site to import.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/bench/pages/two-releases/` (a store with releases v0.2.0 and v0.3.0 for one recipe, all fields within tolerance), when `fid-outreach bench-pages --store store/ --recipes recipes/ --machines machines.toml --out out/ --lint-rules claim-rules.toml --bench-prefix https://example.org/bench/ --json` runs, then it exits 0 and `out/bench/<id>/index.md` shows both values for every field and the journal link with its digest.
- **AC-2** Given `regressed/` (a `lower`-is-better field going from 1.20 to 1.35 with tolerance 0.10 abs), when the generator runs, then the page marks that field `regressed` with change `+0.15` and shows 1.35.
- **AC-3** Given `break/` (the machine registry records a comparability break between the releases), when the generator runs, then the field is marked `not comparable` and not `regressed`.
- **AC-4** Given `not-ok/` (the newest record has status `failed`), when the generator runs, then the page shows `failed` with its reason and no value for the newest release, and the previous value appears only in the previous-release column.
- **AC-5** (error path) Given `lint-fail/` (a recipe title containing "certified" with no designator or link), when the generator runs, then it exits 1 with the lint's CL001 finding and `out/` is empty.
- **AC-6** (determinism) Given the same store, when the generator runs twice, then the output folders are byte-identical.

## Design

- **Command:** `fid-outreach bench-pages --store <dir> --recipes <dir> --machines <file> --out <dir> --lint-rules <file> --bench-prefix <url> [--json]`. Project tooling; nearest class W1 (writes only inside `--out`).
- **Output:** Markdown pages with front matter for OUTR.SITE.GEN to import: `bench/index.md` (per kind), `bench/<id>/index.md` (latest), `bench/<id>/<release>/index.md`; a `bench/manifest.json` listing every page with its source record digests.
- **Algorithm:** read `index.json` for release order; for each recipe, pick the newest record per release (following `supersedes`); compare each field with the previous release's `ok` value using exact decimal arithmetic; regression if worse beyond tolerance (`abs`: difference > tolerance; `rel`: difference / |previous| > tolerance); include kind fragments verbatim; write to a temporary folder; run the claim lint in-process on it with the given rules and benchmark prefix; on success move the folder to `--out`.
- **Errors:** `store_invalid`, `recipe_missing` (a record whose recipe file is gone), `lint_failed`; exit 0, 1 lint or comparison findings that block output, 2 unreadable input.
- **Determinism:** D0; no clock (dates come from records), sorted iteration, BTreeMap only.
- **Crate:** `fid-outreach`, module `bench::pages`; calls the `claim_lint` module.

## Test fixtures

Synthetic stores under `fixtures/outr/bench/pages/` (`two-releases`, `regressed`, `break`, `not-ok`, `lint-fail`) with hand-written result records, a machine registry fixture, and a copy of the outreach rule file; CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-outreach`, add module `bench::pages` and subcommand `fid-outreach bench-pages --store <dir> --recipes <dir> --machines <file> --out <dir> --lint-rules <file> --bench-prefix <url> [--json]`. Inputs are the result store written by `bench::harness` (`bench-result@1` records, `index.json` with release order) and the recipes (`bench-recipe@1`).
>
> For each recipe write `bench/<id>/index.md` (latest release) and `bench/<id>/<release>/index.md`, plus `bench/index.md` grouped by kind and `bench/manifest.json`. Each page shows recipe ID and title, dataset ID and digest, machine `{id, profile, host}`, release, run date, source (argv or bundle selector), a link to the stored journal with its SHA-256, and per field the current and previous release values with units. Use exact decimal arithmetic. Mark a field `regressed` (with the signed change) when it is worse than the previous `ok` value by more than its tolerance in the recipe's `better` direction (`abs` or `rel`); mark `not comparable` when the machine registry records a comparability break between the two releases. Show non-`ok` results with status and reason and no number; never substitute an older value. Include kind fragment files listed in the record verbatim. URL paths must stay stable.
>
> Write into a temp folder, run the existing `claim_lint` module on it with `--lint-rules` and `--bench-prefix`; on any error exit 1 and leave `--out` empty; otherwise move the folder into place.
>
> Tests with fixtures under `fixtures/outr/bench/pages/`: two releases within tolerance (both values and journal link shown), regression (1.20 to 1.35, tolerance 0.10 abs, marked `regressed` with +0.15), comparability break (`not comparable`), newest result failed (no number, status shown), lint failure (exit 1, empty output), and byte-identical output across two runs.
>
> Constraints: no panics, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; no clock reads; BTreeMap rather than HashMap; write only inside `--out`; allowlisted dependency licences only. Write page text in plain words and avoid claim wording the lint forbids.

## Open questions

- The page front-matter fields that OUTR.SITE.GEN imports are not fixed yet; this leaf picks `title`, `kind`, `recipe_id`, `release` and SITE.GEN may rename them.
- Per the coordinator's note, ECO.RULES.VOCAB is to become the one lint engine with OUTR supplying the outreach rule set; OUTR.BRAND.LINT currently specifies its own engine in `fid-outreach`. This leaf calls whatever `claim_lint` entry point OUTR.BRAND.LINT provides.

## Children

_Leaf: none._
