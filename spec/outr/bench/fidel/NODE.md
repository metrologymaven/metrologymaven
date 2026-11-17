---
id: OUTR.BENCH.FIDEL
title: Format fidelity benchmarks
level: 3
parent: OUTR.BENCH
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.BENCH-001]
covers: []
depends_on: [OUTR.BENCH.RUN]
uses_artifacts: [IO.CORP, ECO.DATA.BENCH]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.BENCH.FIDEL: Format fidelity benchmarks

## Purpose

Published round-trip fidelity numbers for each supported format (QIF, mesh, point cloud, CAD, DMIS and others as they land): how many corpus files read, round-trip and compare equal, against the corpus total, with every failing file listed. It makes the clean-room format work measurable (pattern 13 of the design lessons) by publishing the fidelity reports IO.CORP already generates for each release, never by re-scoring them.

## Scope

**In:**
- The `[fidelity]` recipe section, its validator and its result fragment
- One recipe per format with a reader, reading the release's fidelity report from the release-numbers bundle
- The comparison rule of each format, stated in our own words in the recipe
- Listing of failing files, with files whose licence forbids public listing counted but not named

**Out** (and which node owns it instead):
- Corpora, manifests, licences per file and fuzzing (owned by `IO.CORP.MANIF`, `IO.CORP.FUZZ`)
- Scoring and the per-release fidelity report (owned by `IO.CORP.SCORE`, `IO.CORP.GATE`)
- The common recipe format, harness and pages (owned by `OUTR.BENCH.RUN`)

## Requirements

- **REQ-OUTR.BENCH.FIDEL-001** For each format with a registered reader, there shall be one fidelity recipe whose source is the release-numbers bundle entry holding that format's IO.CORP fidelity report.
- **REQ-OUTR.BENCH.FIDEL-002** The fidelity fragment shall report, against the corpus total, the number of files read without error, the number that round-trip (or `not applicable` when the format has no writer), and the number that compare equal after the format's normaliser.
- **REQ-OUTR.BENCH.FIDEL-003** The `[fidelity]` section shall state the comparison rule in our own words (at most 500 characters), naming the coordinate tolerance in metres and which entity kinds are compared, and the validator shall reject a section without it.
- **REQ-OUTR.BENCH.FIDEL-004** The fragment shall name the corpus, its version and its SHA-256 digest, and list every failing file by its manifest identifier.
- **REQ-OUTR.BENCH.FIDEL-005** If a failing file's manifest entry does not allow public listing, then the fragment shall count it under `withheld (licence)` instead of naming it, and the total shall still include it.
- **REQ-OUTR.BENCH.FIDEL-006** Hostile corpus entries shall be reported in a separate row (expected error matched or not) and shall never be counted in the read, round-trip or equal numbers.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/bench/fidel/stl/` (a release-numbers bundle with an STL fidelity report of 20 files: 19 read, 18 round-trip, 17 equal, plus 2 hostile entries with matching errors), when `fid-outreach bench-fidel fragment --recipe fidel-stl.toml --bundle release-numbers.json` runs, then the fragment shows `19 / 20`, `18 / 20`, `17 / 20`, a hostile row `2 / 2 matched`, the corpus name, version and digest, and three failing identifiers.
- **AC-2** Given `no-writer/` (a format with no writer), when the fragment is written, then the round-trip cell reads `not applicable`.
- **AC-3** Given `withheld/` (one failing file whose manifest marks it not listable), when the fragment is written, then that file is absent from the list and `withheld (licence): 1` appears.
- **AC-4** (error path) Given `no-rule.toml` (a `[fidelity]` section without `comparison_rule`), when `fid-outreach bench-recipe check` runs with this validator registered, then it exits 1 with `comparison_rule_missing`.
- **AC-5** (error path) Given a bundle without the selected format's report, when the fragment is written, then the command exits 1 with `report_missing` and writes nothing.
- **AC-6** (coverage) Given the committed format listing (`fid import list-formats --json` output) and `bench/recipes/fidelity/`, when the coverage test runs, then every format with a reader has exactly one recipe.

## Design

- **Command:** `fid-outreach bench-fidel fragment --recipe <file> --bundle <file> [--out <file>]`, also reached through the harness fragment hook; validation through the `bench-recipe check` kind registry (kind `fidelity`). Project tooling; nearest class R0.
- **Schema `[fidelity]`:** `format` (registry format ID), `corpus` (manifest name), `comparison_rule` (own words, max 500 characters), `coord_tolerance_m` (decimal string), `entities` (list of entity kinds compared); unknown keys rejected.
- **Data read:** the IO.CORP fidelity report inside the bundle: per file `sha256`, accepted or rejected, `roundtrip_equal`, `expected_error_match`, manifest identifier and listability flag; corpus name, version and digest.
- **Algorithm:** select the report by `source.selector`; count per file class with exact integers; sort failing identifiers; render a Markdown table plus a failing-file list.
- **Errors:** `report_missing`, `comparison_rule_missing`, `unknown_key`, `format_unknown`.
- **Determinism:** D0.
- **Crate:** `fid-outreach`, module `bench::fidelity`; recipes in the outreach repository under `bench/recipes/fidelity/`.

## Test fixtures

Synthetic release-numbers bundles and fidelity reports under `fixtures/outr/bench/fidel/` (`stl`, `no-writer`, `withheld`, `no-rule.toml`), shaped like the IO.CORP report, hand-written, CC-BY-4.0; a committed format-listing fixture for the coverage test.

## Build prompt

> In the Rust crate `fid-outreach`, add module `bench::fidelity`: a `KindValidator` for kind `fidelity` registered with `bench::recipe`, a fragment writer `fid-outreach bench-fidel fragment --recipe <file> --bundle <file> [--out <file>]` exposed to `bench::harness` through its fragment hook, and recipes under `bench/recipes/fidelity/` (one `bench-recipe@1` file per format with a reader; source `release_numbers`).
>
> `[fidelity]` has `format`, `corpus`, `comparison_rule` (required, own words, at most 500 characters: say the coordinate tolerance in metres and which entity kinds are compared), `coord_tolerance_m` and `entities`; reject unknown keys.
>
> The fragment reads the format's fidelity report from the release-numbers bundle (per-file records with manifest ID, accepted, round-trip equality, expected-error match and a listability flag; corpus name, version, digest) and shows, against the corpus total: files read without error, files that round-trip (`not applicable` with no writer), files equal after normalisation. Hostile entries go in their own row (`matched / total`) and are never counted as read. List failing files by manifest ID, sorted; files not listable by licence are counted as `withheld (licence): N` and not named. Missing report: exit 1 `report_missing`, write nothing.
>
> Tests with fixtures under `fixtures/outr/bench/fidel/`: STL counts 19/20, 18/20, 17/20 with hostile row 2/2; no writer gives `not applicable`; withheld file counted, not named; missing `comparison_rule` fails validation; missing report exits 1; a coverage test that every format with a reader in the committed format listing has exactly one recipe.
>
> Constraints: no panics, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; integer counts only, no re-scoring; BTreeMap rather than HashMap; no network; allowlisted dependency licences only. Write comparison rules in your own words; never quote a format specification.

## Open questions

- The decomposition hint (one child per format family) is not needed: per-format work is one recipe file each, and the mechanics are shared.
- Corpus files under restrictive licences may be usable for testing but not for public listing of failures. REQ-005 assumes the IO.CORP report carries a listability flag per file; if it does not, that is a cross-domain issue for IO.CORP.MANIF and IO.CORP.GATE.
- The former code dependency on IO.CORP is an artifact read (fidelity reports via the ECO.DATA.BENCH bundle) and is now `uses_artifacts` (CONSTITUTION 2.1, D-034).

## Children

_Leaf: none._
