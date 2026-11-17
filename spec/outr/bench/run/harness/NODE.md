---
id: OUTR.BENCH.RUN.HARNESS
title: Release re-run harness and result store
level: 4
parent: OUTR.BENCH.RUN
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.BENCH.RUN-002, REQ-OUTR.BENCH.RUN-003]
covers: []
depends_on: [OUTR.BENCH.RUN.RECIPE]
uses_artifacts: [ECO.REL.SIGN, ECO.DATA.BENCH, PLAT.HW.REFMACH]
side_effect: W1
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.BENCH.RUN.HARNESS: Release re-run harness and result store

## Purpose

When a release is tagged, run every valid recipe against that release and append the results, with their journals, to a result store that keeps every earlier release's results. It reuses numbers other tooling already produced for the release (the release-numbers bundle) and runs the released binaries only for recipes whose source is a command, so the project never measures the same thing twice.

## Scope

**In:** the `bench-run` command; fetching and checksum-verifying the release binaries and the release-numbers bundle; executing `release_numbers` and `command` sources; the result record format; the append-only result store layout; storing journals next to results.
**Out:** the recipe format (`OUTR.BENCH.RUN.RECIPE`); pages and regression display (`OUTR.BENCH.RUN.PAGES`); the release-numbers bundle and its benchmark runs (`ECO.DATA.BENCH`, `PLAT.HW.RUN`); reference machines (`PLAT.HW.REFMACH`); CI scheduling (the project's CI configuration, `ECO.CI`).

## Requirements

- **REQ-OUTR.BENCH.RUN.HARNESS-001** When `bench-run` is given a release tag, the harness shall run every recipe that passes the recipe validator and whose `since_release` is not later than that tag, and write one result record per recipe under `results/<release>/`.
- **REQ-OUTR.BENCH.RUN.HARNESS-002** Before running a `command` source, the harness shall verify the release binary's SHA-256 against the release checksum file, and if it does not match, then it shall stop and run no recipe.
- **REQ-OUTR.BENCH.RUN.HARNESS-003** Each result record shall carry the recipe ID and its SHA-256, the release tag, the run date (UTC), the dataset ID and digest, the machine ID and its profile and host as listed in the reference-machine registry, each field's value and unit, a status, and the path and SHA-256 of the stored journal.
- **REQ-OUTR.BENCH.RUN.HARNESS-004** If a recipe source yields no journal, then the result record shall have status `not_publishable` with reason `no_journal`.
- **REQ-OUTR.BENCH.RUN.HARNESS-005** If a recipe's `machine_profile` is not `any` and differs from the profile of the machine the harness runs on, then the harness shall not run a `command` source for it and shall record status `not_run` with reason `wrong_machine`.
- **REQ-OUTR.BENCH.RUN.HARNESS-006** The result store shall be append-only: if a result record for the same release and recipe exists, then the harness shall refuse to overwrite it unless `--supersede <reason>` is given, in which case it shall keep the old record and write the new one with the reason and a `supersedes` link.
- **REQ-OUTR.BENCH.RUN.HARNESS-007** If a recipe's run fails or times out, then the harness shall record status `failed` with the exit code or `timeout`, continue with the other recipes, and exit non-zero at the end.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/bench/harness/ok/` (two recipes, a stub release folder with a stub `fid` script that prints fixed JSON and writes a journal, its checksum file, and a release-numbers bundle), when `fid-outreach bench-run --release v0.3.0 --release-dir rel/ --recipes recipes/ --store store/ --machine ci-any --machines machines.toml --json` runs, then it exits 0 and writes `store/results/v0.3.0/<id>.json` for both recipes with status `ok` and stored journals whose digests match the records.
- **AC-2** (error path) Given `bad-checksum/` (the stub binary differs from the checksum file), when the harness runs, then it exits 2 with `checksum_mismatch` and writes nothing.
- **AC-3** Given `no-journal/` (a `release_numbers` recipe whose `journal_selector` points to nothing), when the harness runs, then the record has status `not_publishable` and reason `no_journal`.
- **AC-4** Given `wrong-machine/` (a `command` recipe with `machine_profile = "minimum"` and `--machine ci-any` whose profile is `any`), when the harness runs, then the record has status `not_run` and reason `wrong_machine`.
- **AC-5** Given `existing/` (a store already holding a record for the release and recipe), when the harness runs without `--supersede`, then it exits 1 with `result_exists`; with `--supersede "fixed dataset mirror"`, both records exist and the new one names the old one in `supersedes`.
- **AC-6** Given `failing/` (the stub binary exits 3 for one recipe), when the harness runs, then that record has status `failed` with exit code 3, the other recipe has status `ok`, and the harness exits 1.

## Design

- **Command:** `fid-outreach bench-run --release <tag> --release-dir <dir> --recipes <dir> --store <dir> --machine <id> --machines <file> [--supersede <reason>] [--timeout-s <n>] [--json]`. Project tooling; nearest class W1 (writes only inside the given store folder).
- **Result record `bench-result@1`:** `recipe_id`, `recipe_sha256`, `release`, `run_date`, `dataset {id, sha256}`, `machine {id, profile, host}`, `fields [{name, unit, value}]` (value as decimal string as produced by the source), `status` (`ok`, `failed`, `not_run`, `not_publishable`), `reason`, `journal {path, sha256}`, `fragments []` (paths of kind fragments written by kind modules), `supersedes`.
- **Store layout:** `results/<release>/<recipe-id>.json`, `journals/<release>/<recipe-id>.jrn`, `fragments/<release>/<recipe-id>.<kind>.md`; an `index.json` listing releases in tag order.
- **Algorithm:** validate recipes (RECIPE); verify binary checksums; for each recipe in ID order: `release_numbers` sources read the bundle with JSON Pointers; `command` sources run the binary in a fresh temporary working folder with journal recording on, a wall-clock timeout (default 3600 s) and no network for the child where the OS allows it; call the kind module's fragment writer if registered; write the record.
- **Errors:** `checksum_mismatch`, `bundle_missing`, `result_exists`, `machine_unknown`, plus per-recipe statuses; exit 0 all ok or not_run, 1 any failed, not_publishable or refused overwrite, 2 setup failure.
- **Determinism:** N (the results are measured); record serialisation is RFC 8785 canonical JSON.
- **Crate:** `fid-outreach`, module `bench::harness`.

## Test fixtures

Synthetic release folders under `fixtures/outr/bench/harness/` (`ok`, `bad-checksum`, `no-journal`, `wrong-machine`, `existing`, `failing`) with a stub binary written for the tests (a small script or test binary printing fixed JSON), a stub release-numbers bundle and a machine registry file; CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-outreach` (outreach tooling), add module `bench::harness` and subcommand `fid-outreach bench-run --release <tag> --release-dir <dir> --recipes <dir> --store <dir> --machine <id> --machines <file> [--supersede <reason>] [--timeout-s <n>] [--json]`. It uses the existing `bench::recipe` validator (recipes in schema `bench-recipe@1`).
>
> Behaviour: verify each release binary's SHA-256 against the release checksum file in `--release-dir` (stop with `checksum_mismatch`, exit 2, write nothing). For every valid recipe with `since_release` not later than the tag, in recipe-ID order: if the source is `release_numbers`, read values and the journal reference from the release-numbers bundle by JSON Pointer; if it is `command`, run the named release binary with journal recording in a fresh temp folder with a timeout. If the recipe's `machine_profile` is not `any` and differs from the profile of `--machine` in the machines file, record `not_run`/`wrong_machine` instead of running a command. If no journal results, record `not_publishable`/`no_journal`. Failures and timeouts are recorded as `failed` and the run continues.
>
> Write `bench-result@1` records (RFC 8785 JSON) with recipe ID and digest, release, UTC run date, dataset, machine `{id, profile, host}`, field values as decimal strings with units, status, reason, stored journal path and digest, fragment paths and `supersedes`. Store layout: `results/<release>/<id>.json`, `journals/<release>/<id>.jrn`, `fragments/<release>/`, `index.json`. Never overwrite a record: exit 1 with `result_exists` unless `--supersede <reason>`, which keeps the old record and links it.
>
> Tests with fixtures under `fixtures/outr/bench/harness/` for: ok run (two records, journal digests match), bad checksum (exit 2, nothing written), no journal (`not_publishable`), wrong machine (`not_run`), existing record with and without `--supersede`, one failing recipe (record `failed` with exit code, exit 1).
>
> Constraints: no panics, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded reads; write only inside `--store`; BTreeMap rather than HashMap; allowlisted dependency licences only. Do not compute any metrology value here: values come only from the release binaries or the release-numbers bundle.

## Open questions

- Whether the released `fid` can record a journal for an arbitrary CLI invocation (CONSTITUTION C4) is assumed; the flag name is owned by AUTO.CLI.
- PLAT.HW result documents and the ECO.DATA.BENCH bundle do not yet say they carry a journal reference; without one, performance results are `not_publishable` under REQ-004. Cross-domain issue for ECO.DATA.BENCH and PLAT.HW.SCHEMA.

## Children

_Leaf: none._
