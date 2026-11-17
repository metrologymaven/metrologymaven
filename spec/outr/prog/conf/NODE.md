---
id: OUTR.PROG.CONF
title: Driver conformance listing status
level: 3
parent: OUTR.PROG
kind: task
status: leaf
phase: P2
size: S
owners: []
claimed_by:
satisfies: [REQ-OUTR.PROG-002, REQ-OUTR.PROG-003]
covers: []
depends_on: [OUTR.PROG.VENDOR]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.PROG.CONF: Driver conformance listing status

## Purpose

The rule and tool that decide what the public driver listing says about each third-party driver: "conformant" only when its latest conformance report for a named release passes and that release is one of the two most recent, otherwise "not verified for current release". The status is computed from intake records at every site build, never typed by hand. It is project tooling, not a product registry command.

## Scope

**In:** the listing input format (intake records with conformance outcomes); the status rule; the `conf-status` command that outputs the listing data for the site.
**Out:** running conformance suites and signing reports (`HAL.SIM`); verifying report signatures at intake and recording intake data (`OUTR.PROG.VENDOR`); rendering the listing page (`OUTR.SITE`); the release list itself (`ECO.REL`).

## Requirements

- **REQ-OUTR.PROG.CONF-001** The tool shall assign status `conformant` to a driver only when its most recent conformance record (by report date) has outcome pass and names a release that is one of the two most recent releases in the given release list.
- **REQ-OUTR.PROG.CONF-002** If a driver's most recent record has outcome fail, or names a release older than the two most recent releases, then the tool shall assign status `not_verified_for_current_release`.
- **REQ-OUTR.PROG.CONF-003** For every driver, the output shall include the release and the report link of the record the status was based on.
- **REQ-OUTR.PROG.CONF-004** If an intake record names a release that is not in the release list, then the tool shall exit with code 2 and name the record.
- **REQ-OUTR.PROG.CONF-005** The tool shall write its output as canonical JSON sorted by driver ID, byte-identical across runs on the same inputs.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/conf/basic` (releases `v1.0.0`, `v1.1.0`, `v1.2.0`; driver `acme-arm` passing on `v1.2.0`; driver `beta-scan` passing on `v1.0.0`; driver `gamma-probe` failing on `v1.2.0` after passing on `v1.1.0`), when `fid-outreach conf-status --releases releases.json --records records/ --json` runs, then `acme-arm` is `conformant` (release `v1.2.0`), `beta-scan` is `not_verified_for_current_release` (release `v1.0.0`), and `gamma-probe` is `not_verified_for_current_release` (release `v1.2.0`).
- **AC-2** Given fixture `fixtures/outr/conf/boundary` (driver passing on `v1.1.0`, the second most recent release), when the tool runs, then the driver is `conformant`.
- **AC-3** (error path) Given fixture `fixtures/outr/conf/unknown-release` (a record naming `v9.9.9`), when the tool runs, then it exits 2, names the record, and writes no output.
- **AC-4** (determinism) Given fixture `basic` with record files in a different directory order, when the tool runs twice, then both outputs are byte-identical.
- **AC-5** Given every driver in fixture `basic`, when the output is read, then each entry has `release` and `report_url` from the deciding record.

## Design

- **Command:** `fid-outreach conf-status --releases <file> --records <dir> [--json] [--out <file>]`. Project tooling; nearest class R0.
- **Inputs:** `releases.json` is an ordered array of release tags, oldest first (exported from the ECO.REL release manifest). Each intake record (`records/*.toml`, written by OUTR.PROG.VENDOR intake) has `driver_id`, `vendor`, `release`, `outcome` (`pass` | `fail`), `report_date` (ISO 8601 date), `report_url`, `report_sha256`.
- **Algorithm:** load releases into an index map; load records in sorted path order; reject unknown releases (exit 2); group by `driver_id`; pick the record with the latest `report_date` (ties broken by the later release index, then by `report_sha256`); apply the status rule using the two highest release indices; emit sorted canonical JSON (RFC 8785).
- **Errors:** `UnknownRelease(record)`, `ParseError(path)`, `InputUnreadable(path)`.
- **Determinism:** D0. No clock: "current" is defined only by the given release list.
- **Crate:** `fid-outreach`, module `conf_status`.

## Test fixtures

Synthetic TOML and JSON fixtures under `fixtures/outr/conf/` (`basic`, `boundary`, `unknown-release`), with fictional vendor and driver names, CC-BY-4.0.

## Build prompt

> Build the `conf-status` subcommand of the Rust crate `fid-outreach` (outreach tooling, not product code): `fid-outreach conf-status --releases <file> --records <dir> [--json] [--out <file>]`.
>
> Purpose: compute the public listing status of third-party instrument drivers. Inputs: `releases.json`, an ordered array of release tags (oldest first); a directory of TOML intake records with `driver_id`, `vendor`, `release`, `outcome` (`pass`|`fail`), `report_date` (YYYY-MM-DD), `report_url`, `report_sha256`.
>
> Rule: for each driver, take its most recent record by `report_date` (ties: later release, then `report_sha256`). Status is `conformant` if that record's outcome is `pass` and its release is one of the two most recent releases in the list; otherwise `not_verified_for_current_release`. Output one entry per driver with `driver_id`, `vendor`, `status`, `release`, `report_url`, sorted by `driver_id`, as RFC 8785 canonical JSON. If any record names a release not in the list, exit 2 naming the record and write nothing. Exit 0 otherwise.
>
> Acceptance tests with fixtures under `fixtures/outr/conf/`: `basic` (acme-arm conformant on v1.2.0; beta-scan not verified, v1.0.0; gamma-probe not verified, failing on v1.2.0 after an earlier pass); `boundary` (pass on the second most recent release is conformant); `unknown-release` exits 2; outputs are byte-identical regardless of file order; every entry carries `release` and `report_url`.
>
> Constraints: no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded parsing, fail closed; no clock and no network; BTreeMap only; allowlisted dependency licences.

## Open questions

- The intake record fields depend on OUTR.PROG.VENDOR and on HAL.SIM's report format; they are fixed here as the minimum the rule needs, and VENDOR's intake must write them.
- "Older than the two most recent releases" is read as "not one of the two most recent"; a maintainer should confirm.

## Children

_Leaf: none._
