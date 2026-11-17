---
id: OUTR.LAUNCH.KIT.CHECK
title: Demo kit manifest and kit check
level: 4
parent: OUTR.LAUNCH.KIT
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.LAUNCH.KIT-001, REQ-OUTR.LAUNCH.KIT-002, REQ-OUTR.LAUNCH.KIT-003, REQ-OUTR.LAUNCH.KIT-004, REQ-OUTR.LAUNCH.KIT-005]
covers: []
depends_on: [OUTR.BRAND.LINT, OUTR.BRAND.REVIEW, OUTR.CONTENT.REPRO.RUN]
uses_artifacts: [ECO.REL.PKG]
side_effect: R0
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.LAUNCH.KIT.CHECK: Demo kit manifest and kit check

## Purpose

Defines the demo kit manifest (`kit.toml`) and the check that a kit is complete and safe to take to an event: every file listed with a matching checksum and nothing unlisted, the scripted demo's journal replaying `IDENTICAL` with the kit's own binary and no network, every device shown either simulated or run by a named human operator with an approval station, and every printed and on-screen file passing the claim lint and the human trade-dress review. Any kit, for any app, is checked by this one command.

## Scope

**In:** the `kit.toml` schema; the `kit-check` subcommand; the device rule; the offline replay step; the CI job definition that runs the replay with networking disabled.
**Out:** kit contents (`OUTR.LAUNCH.KIT.SCAN` and later kits); installers (`ECO.REL.PKG`); simulators (HAL; read only as release files listed in a kit); replay harness and outcome parsing (`OUTR.CONTENT.REPRO.RUN`, reused); lint rules and review gate (`OUTR.BRAND.LINT`, `OUTR.BRAND.REVIEW`).

## Requirements

- **REQ-OUTR.LAUNCH.KIT.CHECK-001** The kit manifest shall list `release`, every file of the kit with its SHA-256, a hardware list, a `devices` list, the demo (`example_project`, `journal`, `fid` binary path inside the kit) and the printed and on-screen material files.
- **REQ-OUTR.LAUNCH.KIT.CHECK-002** If a listed file is missing, hashes differently, or a file in the kit folder is not listed, then the check shall report `file_missing`, `digest_mismatch` or `file_unlisted` naming it.
- **REQ-OUTR.LAUNCH.KIT.CHECK-003** When run with `--run`, the check shall replay the demo journal with the kit's own `fid` binary on a scratch copy of the kit's example project, using only files inside the kit, and report `demo_not_identical` unless the outcome is `IDENTICAL`.
- **REQ-OUTR.LAUNCH.KIT.CHECK-004** If a `devices` entry has `mode = "simulator"` and does not name a simulator file listed in the kit, or has `mode = "operator"` and lacks an `operator` role and `approval_station = true`, or has any other mode, then the check shall report `device_rule` naming the entry.
- **REQ-OUTR.LAUNCH.KIT.CHECK-005** The check shall run the claim lint on every Markdown and HTML material file and the review gate on the kit's trade-dress review record, and report their findings.
- **REQ-OUTR.LAUNCH.KIT.CHECK-006** A CI job shall run `kit-check --run` for every kit in a container with networking disabled, so that offline operation is tested, not assumed.
- **REQ-OUTR.LAUNCH.KIT.CHECK-007** The check shall exit 0 with no findings, 1 with any finding, and 2 when the kit folder or manifest cannot be read.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/launch/kit/ok` (manifest, all files with matching digests, a stub `fid` that replays `IDENTICAL`, `devices = []`, a clean one-pager, a passing review record), when `fid-outreach kit-check --kit ok --rules claim-rules.toml --roster roster.txt --checklist checklist.toml --author alice --run` runs with networking disabled, then it exits 0.
- **AC-2** Given fixture `kit/extra-file`, when the check runs, then it reports `file_unlisted`.
- **AC-3** Given fixture `kit/divergent` (stub replays `DIVERGENT`), when the check runs with `--run`, then it reports `demo_not_identical`.
- **AC-4** Given fixture `kit/live-device` (`mode = "live"`) and fixture `kit/operator-no-station`, when the check runs, then each reports `device_rule`.
- **AC-5** Given fixture `kit/sim-device` (a simulator file listed and named), when the check runs, then it reports no `device_rule`.
- **AC-6** Given fixture `kit/lint` (one-pager saying "certified accuracy"), when the check runs, then it reports claim-lint rule CL001.

## Design

- **Command:** `fid-outreach kit-check --kit <dir> --rules <file> --roster <file> --checklist <file> --author <handle> [--run] [--json]`. Project tooling; nearest class R0 (replay runs in a temporary copy that is deleted).
- **`kit.toml`:** `version`, `release`, `[[file]] path, sha256`, `[[hardware]] item, qty`, `[[device]] class, mode ("simulator" | "operator"), simulator_file?, operator_role?, approval_station?`, `[demo] example_project, journal, fid`, `material = [paths]`, `review_record = path`.
- **Replay:** reuses the `content_replay` invocation and Replay Report parsing with the kit's binary and a cleared environment.
- **Errors:** findings as above plus lint and review findings; fatal `KitUnreadable`, `ManifestParse`.
- **Determinism:** D1 (the replay outcome comes from the product binary on one platform).
- **Crate:** `fid-outreach`, module `kit_check`; CI job definition in the outreach repository.

## Test fixtures

Synthetic kits under `fixtures/outr/launch/kit/` with stub binaries and tiny example projects. CC-BY-4.0.

## Build prompt

> Build `fid-outreach kit-check --kit <dir> --rules <file> --roster <file> --checklist <file> --author <handle> [--run] [--json]` in the Rust outreach tooling crate `fid-outreach`, reusing `content_replay` (replay invocation and report parsing), `claim_lint` and `review_gate`. Parse `kit.toml` (`release`; `[[file]] path, sha256`; `[[hardware]]`; `[[device]] class, mode, simulator_file?, operator_role?, approval_station?`; `[demo] example_project, journal, fid`; `material`; `review_record`). Report `file_missing`, `digest_mismatch`, `file_unlisted` (any file in the kit folder not listed, except `kit.toml`); `device_rule` (simulator mode without a listed simulator file; operator mode without `operator_role` and `approval_station = true`; any other mode); with `--run`, replay the demo journal using the kit's own binary on a scratch copy of the example project and report `demo_not_identical` unless `IDENTICAL`; claim-lint all Markdown and HTML material; run the review gate on the record with `--author`. Exit 0/1/2; RFC 8785 JSON report. Add a CI job that runs `kit-check --run` on every kit in a container with networking disabled.
>
> Acceptance tests under `fixtures/outr/launch/kit/` as listed in the spec.
>
> Constraints: no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded reads; the tool itself opens no network connection; BTreeMap only; allowlisted dependency licences.

## Open questions

- Device demos (a Probe kit with the CMM simulator) come after launch-1; this check already enforces the simulator-or-operator rule (REQ-OUTR.LAUNCH-007, D-008) without depending on the simulator's code.

## Children

_Leaf: none._
