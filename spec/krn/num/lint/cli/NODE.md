---
id: KRN.NUM.LINT.CLI
title: fid-knp1-lint command, exit codes and seeded-violation suite
level: 4
parent: KRN.NUM.LINT
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-KRN.NUM.LINT-001, REQ-KRN.NUM.LINT-002, REQ-KRN.NUM.LINT-007]
covers: []
depends_on: [KRN.NUM.LINT.FIND, KRN.NUM.LINT.GRAPH, KRN.NUM.LINT.SYN, KRN.NUM.LINT.TYPED, KRN.NUM.LINT.FLAGS, KRN.NUM.LINT.DATA]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
built_in: [fid-knp1-lint]
built_pr: 432
updated: 2026-11-21
---

# KRN.NUM.LINT.CLI: fid-knp1-lint command, exit codes and seeded-violation suite

## Purpose

The entry point CI calls. It loads the lint config, runs the closure, scans and checks in a fixed order, applies waivers, writes `knp1-findings.json`, and exits non-zero on any unwaived finding or tool failure. It also owns the self-test that proves every rule in the catalogue has a seeded violation that is detected, so a rule cannot silently stop working.

## Scope

**In:**
- `fid-knp1-lint check --config knp1-lint.toml [--waivers knp1-waivers.toml] [--denylist rdl-denylist.json] [--skip-typed] --out knp1-findings.json`
- Run order: GRAPH, SYN, TYPED, FLAGS, DATA, then waivers and report
- Exit codes: 0 no unwaived findings; 1 unwaived findings or fatal rule failures; 2 tool or input error (fail closed)
- `fid-knp1-lint selftest`: runs every rule against `fixtures/knp1/seeded/` and the canary, and fails if any catalogue rule lacks a seeded fixture or is not detected
- A documented CI invocation for `ECO.CI` to wire in

**Out** (and which node owns it instead):
- The checks themselves (`KRN.NUM.LINT.GRAPH`, `KRN.NUM.LINT.SYN`, `KRN.NUM.LINT.TYPED`, `KRN.NUM.LINT.FLAGS`, `KRN.NUM.LINT.DATA`) and the report and waivers (`KRN.NUM.LINT.FIND`)
- Deciding which CI gate runs it and when (`ECO.CI`)

## Requirements

- **REQ-KRN.NUM.LINT.CLI-001** When `check` runs, it shall execute the closure and every check in the fixed order GRAPH, SYN, TYPED, FLAGS, DATA, apply waivers, and write the report even when findings exist.
- **REQ-KRN.NUM.LINT.CLI-002** `check` shall exit 0 only when there are no unwaived findings and no fatal errors, 1 when unwaived findings exist, and 2 on any tool or input error, writing no success report in that case.
- **REQ-KRN.NUM.LINT.CLI-003** `--skip-typed` shall be recorded in the report as `typed: skipped`, and the `selftest` and CI default invocation shall not use it.
- **REQ-KRN.NUM.LINT.CLI-004** When `selftest` runs, it shall fail if any rule ID in the catalogue has no seeded fixture or its seeded fixture does not produce exactly the expected finding.
- **REQ-KRN.NUM.LINT.CLI-005** The tool shall make no network access and shall not write outside the `--out` path and a temporary directory.

## Acceptance criteria

- **AC-1** Given `fixtures/knp1/cli/clean-ws/`, when `fid-knp1-lint check` runs, then exit code 0 and a report with zero unwaived findings.
- **AC-2** Given `fixtures/knp1/cli/dirty-ws/` (one `powi` and one `HashMap`), then exit code 1 and both findings in the report.
- **AC-3** (error path) Given a missing config file, then exit code 2 and no report file.
- **AC-4** Given `fixtures/knp1/seeded/` complete, when `selftest` runs, then exit 0; given the fixture for `KNP1-POW` deleted, then it fails naming `KNP1-POW`.
- **AC-5** Given the same inputs twice, then the two reports are byte-identical.

## Design

- **Registry command(s):** none; binary `fid-knp1-lint` in the tooling crate of the same name. It is a CI tool, not a registry command, so it has no MCP projection or journal event.
- **Data structures:** `knp1-lint.toml` (roots, third-party source dir, exclusions, facade implementation files, flags section, wasm module path); argument parser.
- **Algorithm:** load config, run modules in order, collect findings, call `findings::apply_waivers` and `findings::write_report`, compute the exit code. `selftest` iterates the rule catalogue in ID order and runs the relevant module on each seeded fixture.
- **Errors:** exit code 2 with a typed message (`config_invalid`, `input_missing`, `driver_missing`).
- **Determinism:** D0.
- **Performance budget:** full run excluding TYPED compilation < 30 s on CI [unverified until measured].
- **Crate:** `fid-knp1-lint` (tooling layer).

## Test fixtures

`fixtures/knp1/cli/` (clean and dirty workspaces), `fixtures/knp1/seeded/` (shared with SYN, FLAGS, DATA and TYPED); project licence.

## Build prompt

> In tooling crate `fid-knp1-lint`, add the binary `fid-knp1-lint` with subcommands `check --config <toml> [--waivers <toml>] [--denylist <json>] [--skip-typed] --out <json>` and `selftest`. `check` runs modules `graph`, `syn_scan`, `typed`, `flags`, `data` in that order, applies waivers and writes `knp1-findings.json` via `findings`; exit 0 clean, 1 unwaived findings, 2 tool or input error (no success report). Record `--skip-typed` in the report. `selftest` runs each catalogue rule against `fixtures/knp1/seeded/<rule>/` (and the UNC.RDL.LINT canary for data rules) and fails if a rule has no fixture or is not detected exactly. No network; write only the `--out` file and a temp dir. Tests: AC-1..AC-5 of spec node KRN.NUM.LINT.CLI. Constraints (CONSTITUTION §8): no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`.

## Open questions

- None.

## Children

_Leaf: none._
