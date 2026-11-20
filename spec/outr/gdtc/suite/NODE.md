---
id: OUTR.GDTC.SUITE
title: Public GD&T reference suite bundle and self-scoring
level: 3
parent: OUTR.GDTC
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.GDTC-007, REQ-OUTR.GDTC-008]
covers: []
depends_on: [OUTR.GDTC.GUARD]
uses_artifacts: [UNC.ORS.TRACE, UNC.ORS.RUN, GDT.RES.STATUS, OUTR.GDTC.CASE]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-19
---

# OUTR.GDTC.SUITE: Public GD&T reference suite bundle and self-scoring

## Purpose

The project's GD&T reference suites (`UNC.ORS`, including resolved community cases) are only useful as a public benchmark if anyone can run any software against them without the project's own tools. This leaf packages them as a versioned, software-neutral bundle (open input formats, expected values with tolerances, and the interpretation rules and profile each expected value depends on) and provides a small scorer that compares any software's reported values with the expected ones. The project itself never publishes comparisons against named commercial products, because their licences may forbid benchmarking: users run the suites and publish their own results, on their own channels.

## Scope

**In:**
- Bundle `gdt-suite/v1`: per case the nominal geometry, point data, callout, expected values with tolerances and units, the dialect, edition, profile and rule IDs the expected value assumes, and each rule's status at bundle time
- Bundle versioning and digest, tied to the ORS suite version
- The scorer: reads a results CSV from any software and reports per case pass, fail or missing
- The rule that the project publishes, hosts and aggregates no product comparison, and its check

**Out:**
- Suite and case content and expected values (owned by `UNC.ORS` and `OUTR.GDTC.CASE`)
- The project's own benchmark pages (owned by `OUTR.BENCH`)
- Hosting the download (owned by `OUTR.SITE`)
- The lint over prose (owned by `OUTR.GDTC.GUARD`)

## Requirements

- **REQ-OUTR.GDTC.SUITE-001** The bundle shall hold, per case, nominal geometry and point data in open formats (QIF 3.0 for the callout and features, CSV for points in millimetres), expected values with unit and tolerance, and the dialect, edition, profile ID and rule IDs with versions that the expected value assumes.
- **REQ-OUTR.GDTC.SUITE-002** Each expected value shall carry `interpretation: provisional` or `signed_off`, computed from the status of its rules in the status register at bundle time, and the bundle index shall count each.
- **REQ-OUTR.GDTC.SUITE-003** The bundle shall be released under an open licence with a manifest listing every file with its SHA-256, the ORS suite version and the status register digest, and the same inputs shall give a byte-identical bundle.
- **REQ-OUTR.GDTC.SUITE-004** The scorer shall read a results CSV (case ID, characteristic ID, value, unit), convert units exactly, and report per expected value `pass` (within tolerance), `fail` (outside, with the deviation) or `missing`, plus totals, and shall make no network request.
- **REQ-OUTR.GDTC.SUITE-005** Every scorer report shall state the bundle version and digest and that it is a self-run result, not a project result.
- **REQ-OUTR.GDTC.SUITE-006** The repository check shall fail when any file under the repository or the site source is a scorer report (format `gdt-suite-score/v1`), so the project never hosts or aggregates results of any software, its own excepted (the project's own results go through `OUTR.BENCH`).
- **REQ-OUTR.GDTC.SUITE-007** The bundle's README and the suite page shall state that the project publishes no comparisons against named commercial products, that users run and report their own results, and that users should check their software licence before publishing benchmark results.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/gdtc/suite/ors/` (two synthetic ORS cases and one resolved community case) and a status register, when `fid-outreach gdt-suite bundle --ors ors/ --cases cases/ --status status.json --out out/ --json` runs, then the bundle holds three cases with every REQ-001 field, each expected value labelled from the register, and a manifest whose digests match the files.
- **AC-2** (determinism) Running AC-1 twice gives byte-identical bundles.
- **AC-3** Given `results_ok.csv` (all values within tolerance, one in inches), when `fid-outreach gdt-suite score --bundle out/ --results results_ok.csv --json` runs, then every expected value is `pass` and the report states the bundle digest and the self-run statement.
- **AC-4** Given `results_mixed.csv` (one value outside tolerance, one row missing, one unknown case ID), then one `fail` with its deviation, one `missing`, and the unknown row reported as `unknown_case`.
- **AC-5** (error path) Given a repository fixture tree containing a file in format `gdt-suite-score/v1`, when `fid-outreach gdt-suite repo-check --root tree/ --json` runs, then it exits 1 naming the file.
- **AC-6** Given the bundle of AC-1, then its README contains the REQ-007 statements.

## Design

- **Commands:** `fid-outreach gdt-suite bundle --ors <dir> --cases <dir> --status <file> --out <dir>`, `fid-outreach gdt-suite score --bundle <dir> --results <csv>`, `fid-outreach gdt-suite repo-check --root <dir>`; all `--json`. Project tooling; nearest class W1 for `bundle` and `score` (write inside `--out` or to stdout); `repo-check` only reads.
- **Bundle layout:** `manifest.json`, `README.md`, `LICENSE`, `cases/<case-id>/{case.qif, points.csv, expected.json}`; `expected.json` lists per characteristic `{ value, unit, tolerance, interpretation, profile, rules: [{ rule, version }] }`.
- **Scorer:** unit conversion through the kernel's exact unit catalogue; tolerance check in exact decimals; report `gdt-suite-score/v1` JSON and a plain-text summary.
- **Errors:** `ors_invalid`, `case_invalid`, `results_invalid{line}`, `unknown_case`; exit 0, 1, 2.
- **Determinism:** D0.
- **Crate:** `fid-outreach`, module `gdtc::suite`.

## Test fixtures

Synthetic ORS cases, a resolved case, a status register and results CSVs under `fixtures/outr/gdtc/suite/` (invented values; CC-BY-4.0).

## Build prompt

> In crate `fid-outreach`, add module `gdtc::suite` with subcommands `fid-outreach gdt-suite bundle --ors <dir> --cases <dir> --status <file> --out <dir>`, `score --bundle <dir> --results <csv>` and `repo-check --root <dir>` (all `--json`; exit 0, 1, 2). `bundle` writes `gdt-suite/v1`: `manifest.json` (every file with SHA-256, ORS suite version, status register digest), `README.md` (states that the project publishes no comparisons against named commercial products, that users run and report their own results, and that users should check their software licence before publishing), `LICENSE`, and per case `case.qif` (callout and features, QIF 3.0), `points.csv` (mm) and `expected.json` (value, unit, tolerance, `interpretation` from the status of the listed rules, profile, rules with versions); byte-identical for the same inputs. `score` reads a CSV (case ID, characteristic ID, value, unit), converts units exactly, and reports `pass`/`fail` (with deviation)/`missing` per expected value, `unknown_case` rows and totals as `gdt-suite-score/v1` JSON plus a text summary, naming the bundle version and digest and stating it is a self-run result; no network. `repo-check` fails naming any file in format `gdt-suite-score/v1` under the root. Tests per AC-1..AC-6 with fixtures under `fixtures/outr/gdtc/suite/`. Constraints: no panics, no `unsafe`; deny `unwrap`/`expect`/`panic`; no clock reads; BTreeMap only; never include standards text.

## Open questions

- Nominal geometry is given as QIF features for analytic parts; parts that need freeform nominal surfaces wait for an open freeform exchange choice in `IO`.
- The licence of the bundle follows the ORS suites (REQ-UNC.ORS-006); the exact licence identifier is set there.

## Children

_Leaf: none._
