---
id: QUAL.VULN.TRIAGE
title: Vulnerability records, triage timers and embargo checks
level: 3
parent: QUAL.VULN
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-QUAL.VULN-003, REQ-QUAL.VULN-004, REQ-QUAL.VULN-006, REQ-QUAL.VULN-007, REQ-QUAL.VULN-008]
covers: []
depends_on: []
uses_artifacts: [QUAL.SBOM.VEX, ECO.REL.PROC.NOTES]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# QUAL.VULN.TRIAGE: Vulnerability records, triage timers and embargo checks

## Purpose

Holds one private, structured record per reported or dependency-derived vulnerability and checks the process around it: acknowledgement and assessment within the target times, a versioned severity score and affected versions once confirmed, the `actively_exploited` flag with its timestamp, intake of every `affected` VEX finding, and an embargo scan that keeps embargoed details out of public commits, issues and release notes. Records live in a private repository; only derived, non-sensitive files leave it.

## Scope

**In:**
- Record schema `qual-vuln-record/1` (`vuln/records/<id>.toml` in the private security repository): source (`report` or `dependency`), received, acknowledged and assessed times, state (`received|acknowledged|confirmed|rejected|fixed|published`), severity system and version (CVSS v3.1 or v4.0 [unverified choice]) with vector and score, affected and fixed versions, `actively_exploited` with `exploited_set_at`, embargo codewords, values-of-record impact fields, owner
- Timer check against working-day targets, output `vuln/overdue.json`
- Dependency intake: every entry of `sbom/affected.json` has a record whose `source_ref` names it
- Exploited export `vuln/exploited.json` (ID, set time, severity, affected versions) for `QUAL.CRA.REPORT`
- Embargo scan of public text (commit messages, PR and issue export, release-notes draft) for record IDs and codewords of records under embargo

**Out** (and which node owns it instead):
- Advisory format and feed (owned by `QUAL.VULN.ADVIS`)
- VEX statements (owned by `QUAL.SBOM.VEX`)
- Regulatory notifications (owned by `QUAL.CRA.REPORT`)
- Building patch releases (owned by `ECO.REL.PROC.FLOW`)

## Requirements

- **REQ-QUAL.VULN.TRIAGE-001** If a record in state `received` has no acknowledgement time within 3 working days of its received time, or has no assessment within 10 working days, at the check time, then the timer check shall list it in `vuln/overdue.json` and exit non-zero [unverified targets].
- **REQ-QUAL.VULN.TRIAGE-002** If a record in state `confirmed` or later lacks a severity system name and version, a score, a vector or a non-empty affected-version list, then the check shall fail and name the field.
- **REQ-QUAL.VULN.TRIAGE-003** Every record in state `confirmed` or later shall carry `actively_exploited` as true or false, and when it is true shall carry `exploited_set_at`; the check shall fail otherwise.
- **REQ-QUAL.VULN.TRIAGE-004** If an entry of `sbom/affected.json` has no record whose `source_ref` equals its vulnerability ID and component, then the intake check shall fail and name it.
- **REQ-QUAL.VULN.TRIAGE-005** When the embargo scan runs over a public-text export, it shall fail and name the location of every occurrence of the ID or a codeword of a record whose embargo end is later than the check time.
- **REQ-QUAL.VULN.TRIAGE-006** The tool shall write `vuln/exploited.json` with every record whose `actively_exploited` is true, and the file shall contain no vector, reproduction or reporter detail.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/qual/vuln/triage/ok/` (5 records in different states, `affected.json` with 1 entry, check time 2027-03-10T09:00Z), when `cargo xtask qual vuln check --records records/ --affected affected.json --now 2027-03-10T09:00:00Z --out out/` runs, then it exits 0 and `out/vuln/exploited.json` equals the golden file.
- **AC-2** (error path) Given `late-ack/` (received Monday 2027-03-01, no acknowledgement, check time Friday 2027-03-05), then it exits 1 and `overdue.json` names the record.
- **AC-3** (error path) Given `no-score/` (confirmed without a vector), then it exits 1 naming `severity.vector`.
- **AC-4** (error path) Given `exploited-no-time/`, then it exits 1.
- **AC-5** (error path) Given `intake-gap/` (affected entry with no record), then it exits 1 naming the entry.
- **AC-6** (error path) Given `embargo/` (a commit message containing codeword `BLUE-HERON` of a record under embargo until 2027-04-01), when `qual vuln embargo-scan --records records/ --public public.jsonl --now 2027-03-10T09:00:00Z` runs, then it exits 1 naming the commit; with `--now 2027-04-02T00:00:00Z` it exits 0.

## Design

- **Registry command(s):** none; repository tools `cargo xtask qual vuln check --records <dir> --affected <json> --now <rfc3339> --out <dir>` (W1) and `cargo xtask qual vuln embargo-scan --records <dir> --public <jsonl> --now <rfc3339>` (R0). Both run in the private security repository's CI.
- **Data structures:** `Record { id: "FID-YYYY-NNNN", source: Report|Dependency, source_ref, received, acknowledged, assessed, state, severity: Option<{system, version, vector, score}>, affected, fixed, actively_exploited: Option<bool>, exploited_set_at, embargo_until, codewords, vor_impact: Option<{algorithm_ids, reevaluate}>, owner }`; public-text export lines `{kind, location, text}`.
- **Algorithm:** working days are Monday to Friday UTC; whole-word, case-insensitive codeword matching; outputs sorted by ID.
- **Errors:** `VULN-PARSE`, `VULN-FIELD(record, field)`, `VULN-OVERDUE(record)`, `VULN-INTAKE(entry)`, `VULN-EMBARGO(location)`.
- **Determinism:** D0; check time explicit.
- **Performance budget:** under 2 s for 1,000 records and 50,000 public-text lines.
- **Crate:** `fid-xtask` (module `qual::vuln::triage`).

## Test fixtures

`fixtures/qual/vuln/triage/{ok,late-ack,no-score,exploited-no-time,intake-gap,embargo}/` with synthetic records and codewords; CC-BY-4.0.

## Build prompt

> In `fid-xtask`, add module `qual::vuln::triage` with `qual vuln check --records <dir> --affected <json> --now <rfc3339> --out <dir>` and `qual vuln embargo-scan --records <dir> --public <jsonl> --now <rfc3339>`. Record schema `qual-vuln-record/1` (fields: id, source report|dependency, source_ref, received, acknowledged, assessed, state, severity {system, version, vector, score}, affected, fixed, actively_exploited, exploited_set_at, embargo_until, codewords, vor_impact {algorithm_ids, reevaluate}, owner). Check: acknowledgement within 3 and assessment within 10 working days (Mon-Fri UTC) at `--now`, writing `vuln/overdue.json`; confirmed records need severity and affected versions and an explicit actively_exploited (with set time when true); every `sbom/affected.json` entry has a record by source_ref; write `vuln/exploited.json` with id, set time, severity score and affected versions only. Embargo scan: whole-word, case-insensitive match of IDs and codewords of records with embargo_until > now in the public-text export; fail naming locations. Treat all text as untrusted; no panics, deny unwrap/expect, BTreeMap only. Tests: AC-1..AC-6 of spec node QUAL.VULN.TRIAGE.

## Open questions

- The severity system (CVSS v3.1 or v4.0) is a maintainer choice; the schema records system and version so either works [unverified].
- Producing the public-text export (commits, PR and issue titles and bodies, release-notes draft) is a CI job; `ECO.REL.PROC.NOTES` already produces the PR export it can reuse.

## Children

_Leaf: none._
