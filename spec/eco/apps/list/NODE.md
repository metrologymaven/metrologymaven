---
id: ECO.APPS.LIST
title: App list file and its check
level: 3
parent: ECO.APPS
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-ECO.APPS-001, REQ-ECO.APPS-007]
covers: []
depends_on: [ECO.COMM]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.APPS.LIST: App list file and its check

## Purpose

One machine-readable file that names every focused app the project ships (D-004: Inspect, Probe, Network, Report), with a stable identifier, a one-line job, an owning maintainer and a phase, plus a repository check that keeps the file well formed and refuses to add or remove an app without an RFC that ran the longer comment period. Everything else that talks about "the apps" (manifests, release packaging, parity status pages) reads this file instead of keeping its own list.

## Scope

**In:**
- The app list file and its schema
- Retired identifiers (an ID once used is never reused)
- The `apps-check` repository tool: schema, uniqueness, retirement and RFC-link rules

**Out:**
- What each app enables (owned by `ECO.APPS.MANIFEST` and the per-app nodes `ECO.APPS.INSPECT`, `ECO.APPS.PROBE`, `ECO.APPS.NETWORK`, `ECO.APPS.REPORT`)
- The RFC files and their comment-period timer (owned by `ECO.COMM`)
- Running the check on PRs (owned by `ECO.CI`)

## Requirements

- **REQ-ECO.APPS.LIST-001** The app list shall give each app an `id` matching `^[a-z][a-z0-9-]{1,15}$`, a `job` of at most 120 characters, an `owner` (a maintainer handle or the literal `vacant`), and a `phase` from CONSTITUTION §7; release status is not kept here (it belongs to the app manifest).
- **REQ-ECO.APPS.LIST-002** If two entries share an `id`, or an entry uses an `id` listed under `retired`, then the check shall fail and name the identifier.
- **REQ-ECO.APPS.LIST-003** When a change adds or removes an app entry relative to the base revision, the check shall fail unless the entry (or, for a removal, the matching `retired` row) names an RFC or decision record whose recorded comment period is at least 7 days and whose status is `accepted`.
- **REQ-ECO.APPS.LIST-004** When an entry's `owner` is `vacant`, the check shall emit a warning naming the app, not a failure.
- **REQ-ECO.APPS.LIST-005** The check shall run offline, read only the list file, the base-revision copy of it and the decision/RFC records, and exit 0 on success and 1 on any failure.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/apps/valid.toml` (the four D-004 apps, all fields filled), when `cargo xtask apps-check --list fixtures/apps/valid.toml` runs, then it exits 0 and prints no findings.
- **AC-2** (error path) Given `fixtures/apps/dup-id.toml` (two entries with `id = "inspect"`), when the check runs, then it exits 1 with finding `APPS-DUP inspect`.
- **AC-3** (error path) Given `fixtures/apps/reused.toml` (an entry whose id is in `retired`), when the check runs, then it exits 1 with finding `APPS-RETIRED <id>`.
- **AC-4** (RFC rule) Given base `fixtures/apps/valid.toml` and head `fixtures/apps/added-no-rfc.toml` (a fifth app with no `rfc` field), when `cargo xtask apps-check --list <head> --base <base> --records fixtures/apps/records/` runs, then it exits 1 with finding `APPS-NO-RFC`; given head `fixtures/apps/added-rfc-3day.toml` pointing at a record with a 3-day period, it exits 1 with `APPS-RFC-PERIOD`; given `fixtures/apps/added-rfc-ok.toml` pointing at an accepted 7-day record, it exits 0.
- **AC-5** (warning) Given `fixtures/apps/vacant.toml`, when the check runs, then it exits 0 and prints `warn APPS-VACANT <id>` once per vacant app.
- **AC-6** (determinism) Running AC-1 to AC-5 twice produces byte-identical output.

## Design

- **Registry command(s):** none. This is a repository tool (`cargo xtask apps-check`), not a product command; `side_effect: R0` records that it only reads files.
- **Data structures:** `apps.toml` with `[[app]]` tables (`id`, `job`, `owner`, `phase`, `decision` or `rfc`) and a `[[retired]]` table (`id`, `retired_in`, `rfc`). Initial content: `inspect` (scan-to-CAD inspection), `probe` (CMM programming, simulation and execution), `network` (large-volume, multi-instrument networks), `report` (reporting and SPC), each with `decision = "D-004"`. Proposed phases: report P1a, inspect P1b, probe P2, network P3 [unverified; set by the per-app nodes and reviewed in the PR].
- **Algorithm:** parse with a bounded TOML reader (reject files over 64 KiB); validate fields; collect findings into a list sorted by (code, id); if `--base` is given, diff the ID sets and look up each added or removed ID's `rfc`/`decision` record in `--records`, reading only its frontmatter `status` and `comment_period_days` (the RFC record format is owned by ECO.COMM; D-004 predates that field, so records listed in an `grandfathered` array inside the file are accepted).
- **Errors:** `APPS-SCHEMA`, `APPS-DUP`, `APPS-RETIRED`, `APPS-NO-RFC`, `APPS-RFC-PERIOD`, `APPS-RFC-STATUS`, `APPS-IO`.
- **Determinism:** D0; pure function of the input files, sorted output, no clock.
- **Performance budget:** under 1 s for a 100-entry list [unverified until measured].
- **Crate:** `fid-xtask` (repository tooling crate).

## Test fixtures

Hand-written TOML files and Markdown decision records under `fixtures/apps/`, written for this leaf (licence: the project's code licence, MIT OR Apache-2.0).

## Build prompt

> In the implementation repository, add an `apps-check` subcommand to the `fid-xtask` crate. It reads `apps.toml` (schema: `[[app]]` with `id` matching `^[a-z][a-z0-9-]{1,15}$`, `job` ≤ 120 chars, `owner` handle or `vacant`, `phase` in P0|P1a|P1b|P2|P3|P4|later, and `decision` or `rfc`; `[[retired]]` with `id`, `retired_in`, `rfc`; optional `grandfathered` list of record IDs). Flags: `--list <file>`, optional `--base <file>`, optional `--records <dir>`. Findings, sorted by code then id: `APPS-SCHEMA`, `APPS-DUP`, `APPS-RETIRED`, `APPS-NO-RFC` (an added or removed app with no record), `APPS-RFC-PERIOD` (record's `comment_period_days` < 7), `APPS-RFC-STATUS` (record not `accepted`), warning `APPS-VACANT`. Exit 0 on success (warnings allowed), 1 on any failure. Seed `apps.toml` with inspect, probe, network and report, each `decision = "D-004"`, and add D-004 to `grandfathered`. Write fixtures `fixtures/apps/{valid,dup-id,reused,added-no-rfc,added-rfc-3day,added-rfc-ok,vacant}.toml` plus records, and tests for AC-1..AC-6 (see the spec node ECO.APPS.LIST). Constraints: no `unwrap`/`expect`/`panic`/`unsafe` in non-test code; bounded input (64 KiB); runs offline; standard error type with the codes above; output byte-identical across runs.

## Open questions

- Release status (alpha, beta, stable) lives only in each app manifest (ECO.APPS.MANIFEST), so this file and the manifest cannot disagree. An app listed here with no manifest yet is reported as `planned` by ECO.PARITY.

## Children

_Leaf: none._
