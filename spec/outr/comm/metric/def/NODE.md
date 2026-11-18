---
id: OUTR.COMM.METRIC.DEF
title: Metric definition file, source list and guard
level: 4
parent: OUTR.COMM.METRIC
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-outreach]
built_pr: 87
satisfies: [REQ-OUTR.COMM.METRIC-001, REQ-OUTR.COMM.METRIC-002, REQ-OUTR.COMM.METRIC-003]
covers: []
depends_on: []
uses_artifacts: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.COMM.METRIC.DEF: Metric definition file, source list and guard

## Purpose

The written definition of every adoption metric and the list of sources the collector may read, each marked public with a reason, plus the guard that refuses any metric without a definition, any source not marked public, and any source that looks like data collected by the product. It is the no-telemetry boundary of adoption measurement, written down once and checked on every change.

## Scope

**In:** `metrics/definitions.toml`; `metrics/sources.toml`; the deny-list of product-data source patterns; the `metric-def check` command; the initial metric set.
**Out:** fetching data (`OUTR.COMM.METRIC.COLLECT`); snapshots and quarterly values (`OUTR.COMM.METRIC.SNAP`); survey data (`OUTR.COMM.SURVEY`); the report (`OUTR.COMM.REPORT`).

## Requirements

- **REQ-OUTR.COMM.METRIC.DEF-001** Each metric definition shall state `id`, `title`, `source` (a source ID), `method` (`http_json`, `git_history` or `manual_public_count`), `selector` where the method needs one, `unit`, `aggregation` per quarter (`last`, `sum` or `max`) and at least one entry in `biases`, and the check shall fail naming every missing field.
- **REQ-OUTR.COMM.METRIC.DEF-002** Each source shall state `id`, `url`, `public = true` and a `public_basis` sentence, and if a source has `public = false` or no basis, then the check shall fail with `source_not_public`.
- **REQ-OUTR.COMM.METRIC.DEF-003** If a source URL or ID matches the deny-list of product-data patterns (telemetry, crash reports, support bundles, update-check logs, licence or activation servers), then the check shall fail with `product_data_source`, whatever its `public` flag says.
- **REQ-OUTR.COMM.METRIC.DEF-004** If a definition names a source ID that is not in the source list, then the check shall fail with `source_unknown`.
- **REQ-OUTR.COMM.METRIC.DEF-005** The initial definitions shall cover release downloads per channel, repository stars, repository forks, distinct contributors per quarter, forum posts per quarter and chat messages in public channels per quarter, each with its known biases (for example mirror downloads and package-manager installs not counted, bots not excluded).

## Acceptance criteria

- **AC-1** Given the committed `metrics/definitions.toml` and `metrics/sources.toml`, when `fid-outreach metric-def check --defs metrics/definitions.toml --sources metrics/sources.toml --json` runs, then it exits 0 and lists the six initial metrics.
- **AC-2** (error path) Given fixture `fixtures/outr/comm/metric/def/no-bias.toml`, when the check runs, then it exits 1 with `field_missing biases`.
- **AC-3** (error path) Given `private-source.toml` (`public = false`), when the check runs, then it exits 1 with `source_not_public`.
- **AC-4** (error path) Given `telemetry-source.toml` (a source URL containing `/telemetry/`, marked public), when the check runs, then it exits 1 with `product_data_source`.
- **AC-5** (error path) Given `unknown-source.toml`, when the check runs, then it exits 1 with `source_unknown`.

## Design

- **Command:** `fid-outreach metric-def check --defs <file> --sources <file> [--json]`. Project tooling; nearest class R0. The collector calls the same validation before it runs.
- **Deny-list:** compiled into the crate (not configurable from the definition files): case-insensitive substrings `telemetry`, `crash`, `support-bundle`, `support_bundle`, `update-check`, `activation`, `licence-server`, `license-server`, `diagnostic`; maintainers may add, never remove, through a pull request to the crate.
- **Errors:** `field_missing`, `source_not_public`, `product_data_source`, `source_unknown`, `duplicate_id`, `parse`.
- **Determinism:** D0.
- **Crate:** `fid-outreach`, module `metrics::def`; files in the outreach repository under `metrics/`.

## Test fixtures

Hand-written TOML under `fixtures/outr/comm/metric/def/` (`no-bias`, `private-source`, `telemetry-source`, `unknown-source`), CC-BY-4.0, using `example.org` URLs.

## Build prompt

> In the outreach repository, write `metrics/definitions.toml` and `metrics/sources.toml`. Definitions (`[[metric]]`: `id`, `title`, `source`, `method` = http_json | git_history | manual_public_count, `selector` (JSON Pointer for http_json), `unit`, `aggregation` = last | sum | max, `biases` list) for: release downloads per channel, repository stars, repository forks, distinct contributors per quarter, forum posts per quarter, public chat messages per quarter, each with honest biases in your own words. Sources (`[[source]]`: `id`, `url`, `public = true`, `public_basis`). Use placeholder hosts until the repository and release hosts are chosen.
>
> In the Rust crate `fid-outreach`, add module `metrics::def` with `fid-outreach metric-def check --defs <file> --sources <file> [--json]`: `field_missing <field>`, `source_not_public`, `source_unknown`, `duplicate_id`, and `product_data_source` for any source whose URL or ID contains (case-insensitive) telemetry, crash, support-bundle, support_bundle, update-check, activation, licence-server, license-server or diagnostic. Keep that deny-list in code, not in the data files. Exit 0, 1 findings, 2 unreadable input. Expose the validation as a library function for the collector.
>
> Tests with fixtures under `fixtures/outr/comm/metric/def/` for each error, and the committed files passing.
>
> Constraints: no panics, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; no network; BTreeMap rather than HashMap.

## Open questions

- The repository and release hosts are not chosen yet (ECO.REPO, ECO.REL); source URLs are placeholders until then.

## Children

_Leaf: none._
