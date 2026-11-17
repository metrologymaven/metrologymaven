---
id: QUAL.EVID.ANOM
title: Known-anomalies list and its check
level: 3
parent: QUAL.EVID
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-QUAL.EVID-007]
covers: []
depends_on: []
uses_artifacts: []
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# QUAL.EVID.ANOM: Known-anomalies list and its check

## Purpose

Keeps the list of known defects that affect values of record as reviewed data, one entry per open issue carrying the label `affects-values-of-record`, with the affected algorithm IDs and a workaround, and fails the release when an open labelled issue is missing from the list. Honest status for regulated users means they can see every known numeric defect in the release they qualify.

## Scope

**In:**
- `release/anomalies.toml` schema `qual-anomalies/1`: issue number, title (untrusted), affected algorithm IDs, affected versions, workaround (or "none"), review date
- Check against an issue export (open issues with labels) taken at the release commit
- Rendered `anomalies.json` (bundle member) and Markdown

**Out** (and which node owns it instead):
- Labelling issues (maintainers)
- Security vulnerabilities (owned by `QUAL.VULN.ADVIS`)

## Requirements

- **REQ-QUAL.EVID.ANOM-001** Each entry shall hold the issue number, affected algorithm IDs (at least one), affected versions and a workaround or the value `none`.
- **REQ-QUAL.EVID.ANOM-002** If an open issue in the export carries the label `affects-values-of-record` and has no entry, then the check shall fail and name the issue.
- **REQ-QUAL.EVID.ANOM-003** If an entry names an issue that is closed in the export, then the check shall warn and the rendered list shall mark it `closed_since_review`.
- **REQ-QUAL.EVID.ANOM-004** The tool shall write `anomalies.json` as canonical JSON sorted by issue number.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/qual/evid/anom/ok/` (3 labelled open issues, 3 entries), when `cargo xtask qual evidence anomalies --list anomalies.toml --issues issues.json --out out/` runs, then it exits 0 and `out/anomalies.json` equals the golden file.
- **AC-2** (error path) Given `unlisted/` (a fourth labelled issue), then it exits 1 naming it.
- **AC-3** Given `closed/` (an entry whose issue is closed), then it exits 0 with a warning and the entry marked `closed_since_review`.
- **AC-4** (error path) Given an entry without algorithm IDs, then it exits 1.

## Design

- **Registry command(s):** none; repository tool `cargo xtask qual evidence anomalies --list <toml> --issues <json> --out <dir>` (W1).
- **Data structures:** `Anomaly { issue, title: Untrusted<String>, algorithm_ids, versions, workaround, reviewed_on }`; issue export `[{number, state, labels, title}]`.
- **Algorithm:** set difference by issue number.
- **Errors:** `ANOM-PARSE`, `ANOM-UNLISTED(issue)`, `ANOM-FIELD(issue)`.
- **Determinism:** D0.
- **Performance budget:** under 1 s.
- **Crate:** `fid-xtask` (module `qual::evid::anom`).

## Test fixtures

`fixtures/qual/evid/anom/{ok,unlisted,closed}/`; synthetic issues; CC-BY-4.0.

## Build prompt

> In `fid-xtask`, add module `qual::evid::anom`: `qual evidence anomalies --list <release/anomalies.toml> --issues <issues.json> --out <dir>`. Entries: issue, title (untrusted), algorithm_ids (≥1), versions, workaround or "none", reviewed_on. Fail when an open issue labelled `affects-values-of-record` has no entry or an entry lacks fields; warn and mark `closed_since_review` for entries whose issue is closed. Write `anomalies.json` (canonical JSON, sorted by issue) and a Markdown page. No network (the issue export is produced by a separate CI step), no panics, deny unwrap/expect. Tests: AC-1..AC-4 of spec node QUAL.EVID.ANOM.

## Open questions

- The label name `affects-values-of-record` must be added to the repository label set (ECO.CI.BOTS or maintainers).

## Children

_Leaf: none._
