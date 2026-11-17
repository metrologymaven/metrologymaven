---
id: OUTR.CONTENT.TUT.CHECK
title: Tutorial coverage and step-output check
level: 4
parent: OUTR.CONTENT.TUT
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.CONTENT.TUT-001, REQ-OUTR.CONTENT.TUT-002, REQ-OUTR.CONTENT.TUT-004, REQ-OUTR.CONTENT.TUT-005]
covers: []
depends_on: [OUTR.CONTENT.PUB, OUTR.CONTENT.REPRO.RUN]
uses_artifacts: [ECO.APPS.LIST, ECO.APPS.MANIFEST, ECO.REL.PKG, ECO.DATA.EX]
side_effect: R0
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.CONTENT.TUT.CHECK: Tutorial coverage and step-output check

## Purpose

The check that keeps tutorials complete and true: every app at alpha or later has a tutorial, the core CLI walkthrough exists, every tutorial states its time and prerequisites, and every command step, run with the release's own `fid` binary on a scratch copy of the example project, prints the expected output shown in the tutorial after normalisation. It is outreach tooling; it runs released binaries and links no product crate.

## Scope

**In:** the `tut-check` subcommand; the tutorial front-matter fields `app`, `topics`, `duration_min` and `prerequisites`; the step and expected-output block convention; the app coverage rule.
**Out:** manifest fields and claim lint (`OUTR.CONTENT.PUB`); the normalisation rules (`OUTR.CONTENT.REPRO.RUN`); replay across releases (`OUTR.CONTENT.REPRO.RUN`); tutorial text (`OUTR.CONTENT.TUT.CORE`, `OUTR.CONTENT.TUT.REPORT`, `OUTR.CONTENT.TUT.INSPECT`, `OUTR.CONTENT.TUT.PROBE`, `OUTR.CONTENT.TUT.NETWORK`).

## Requirements

- **REQ-OUTR.CONTENT.TUT.CHECK-001** If an app in the app list has status `alpha`, `beta` or `stable` in the exported app status and no tutorial has `app:` equal to its ID, then the check shall report `app_without_tutorial` naming the app.
- **REQ-OUTR.CONTENT.TUT.CHECK-002** If no tutorial has `app: core` and `topics` containing all of `cli`, `journal`, `replay` and `verify`, then the check shall report `core_walkthrough_missing`.
- **REQ-OUTR.CONTENT.TUT.CHECK-003** If a tutorial lacks `duration_min` (a positive integer) or any of `prerequisites.release`, `prerequisites.example_project` and `prerequisites.hardware_profile`, then the check shall report `missing_field` naming the file and field.
- **REQ-OUTR.CONTENT.TUT.CHECK-004** When the check runs with `--run`, it shall execute every `sh step` block of each tutorial in document order with the given `fid` binary in a scratch copy of the tutorial's example project, and report `output_mismatch` with file and line when the normalised stdout differs from the following `text expected` block.
- **REQ-OUTR.CONTENT.TUT.CHECK-005** The check shall never modify the content root or the example project directory it was given.
- **REQ-OUTR.CONTENT.TUT.CHECK-006** The check shall exit 0 with no findings, 1 with any finding, and 2 when an input cannot be read.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/tut/ok` (an app list with `report` at alpha, a core tutorial and a report tutorial with matching expected outputs, and the stub binary `fid-stub`), when `fid-outreach tut-check --root content --apps apps.toml --app-status apps-status.json --fid fid-stub --examples ex --run` runs, then it exits 0.
- **AC-2** Given fixture `fixtures/outr/tut/no-app-tut` (`inspect` at alpha, no inspect tutorial), when the check runs, then it reports `app_without_tutorial` for `inspect`.
- **AC-3** Given fixture `fixtures/outr/tut/no-core`, when the check runs, then it reports `core_walkthrough_missing`.
- **AC-4** Given fixture `fixtures/outr/tut/mismatch` (an expected block that differs after normalisation), when the check runs with `--run`, then it reports `output_mismatch` with the block's line.
- **AC-5** Given fixture `fixtures/outr/tut/timestamps` (expected and actual differ only in a UUID and a timestamp), when the check runs with `--run`, then it reports nothing.
- **AC-6** (isolation) Given AC-1, when it finishes, then the content and example directories hash the same as before the run.
- **AC-7** (error path) Given a missing `--apps` file, when the check runs, then it exits 2.

## Design

- **Command:** `fid-outreach tut-check --root <dir> --apps <apps.toml> --app-status <json> [--fid <binary> --examples <dir> --run] [--json]`. Project tooling; nearest class R0 (writes only to a temporary directory that it deletes).
- **Inputs read:** from `apps.toml` (ECO.APPS.LIST) the `[[app]] id` values; from the app status export (ECO.APPS.MANIFEST) `apps[] {id, status}`; tutorials are content files with `kind: tutorial`.
- **Block convention:** a fenced block with info string `sh step` holds one `fid` command line; the next fenced block with info string `text expected` holds its expected stdout. A step without a following expected block is checked only for exit code 0.
- **Algorithm:** parse manifests with the PUB module; apply coverage and field rules; with `--run`, copy `{examples}/{example_project}` to a temp directory, run each step there (timeout 120 s), normalise both texts with the REPRO.RUN `normalise` module, compare; delete the temp directory; sort findings.
- **Errors:** findings `app_without_tutorial`, `core_walkthrough_missing`, `missing_field`, `step_failed`, `output_mismatch`, `step_timeout`; fatal `InputUnreadable`.
- **Determinism:** D1 (outputs of the product binary are per-platform; CI runs on each PLAT Tier 1 OS).
- **Performance budget:** the check itself adds under 1 s per tutorial beyond the commands it runs [unverified until measured].
- **Crate:** `fid-outreach`, module `tut_check`.

## Test fixtures

Synthetic fixtures under `fixtures/outr/tut/` (`ok`, `no-app-tut`, `no-core`, `mismatch`, `timestamps`) with a stub `fid` script that prints canned outputs per argument list, small app lists and status exports. CC-BY-4.0.

## Build prompt

> Build the `tut-check` subcommand of the Rust outreach tooling crate `fid-outreach` (tooling only; it runs the released `fid` binary as a process and links no product crate). Reuse the crate's `content_check` manifest parser and the `normalise` function of the `content_replay` work.
>
> Signature: `fid-outreach tut-check --root <dir> --apps <apps.toml> --app-status <json> [--fid <binary> --examples <dir> --run] [--json]`. Rules: (1) every app ID in `apps.toml` whose status in the status export is `alpha`, `beta` or `stable` needs a tutorial (`kind: tutorial`) with `app: <id>`, else `app_without_tutorial`; (2) a tutorial with `app: core` and `topics` containing `cli`, `journal`, `replay`, `verify` must exist, else `core_walkthrough_missing`; (3) each tutorial needs `duration_min` (positive integer) and `prerequisites.release`, `.example_project`, `.hardware_profile`, else `missing_field`; (4) with `--run`, copy the example project to a temp dir, run each `sh step` block's single `fid` command line there in order (timeout 120 s), and compare normalised stdout with the next `text expected` block (`output_mismatch` with line), or require exit 0 when no expected block follows (`step_failed`). Never modify inputs; always delete the temp dir. Exit 0/1/2 as usual; RFC 8785 JSON report sorted by file, line, code.
>
> Acceptance tests with fixtures under `fixtures/outr/tut/` as listed in the spec (ok, missing app tutorial, missing core walkthrough, mismatch, UUID and timestamp differences ignored, inputs unchanged, missing app list exits 2).
>
> Constraints: no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded reads; kill child processes on timeout; no network; BTreeMap only; allowlisted dependency licences.

## Open questions

- The app status export's field names follow ECO.APPS.MANIFEST (REQ-ECO.APPS.MANIFEST-005), which is still open; only the input mapping here changes if they differ.

## Children

_Leaf: none._
