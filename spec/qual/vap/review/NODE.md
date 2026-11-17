---
id: QUAL.VAP.REVIEW
title: Periodic-review report command
level: 3
parent: QUAL.VAP
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-QUAL.VAP-007]
covers: []
depends_on: [QUAL.IQOQ.RECORD, QUAL.VAP.TPL, CORE.BUS.POL.FS]
uses_artifacts: [QUAL.VULN.ADVIS]
side_effect: W1
determinism: D0
cli: [fid validate make-review-report]
mcp: [validate_make_review_report]
journal: [validate.make_review_report@1]
updated: 2026-11-17
---

# QUAL.VAP.REVIEW: Periodic-review report command

## Purpose

`validate.make_review_report@1` produces the input to a customer's periodic review of their validated system over a date range: every qualification record, every product version change seen in those records, and every security advisory affecting an installed version in that range. Advisories come from a local copy of the advisory feed (imported by the user or kept by the consented update check), so the command stays offline.

## Scope

**In:**
- Command with parameters: from and to dates, export-root handle, name, optional advisory-feed import handle
- Sections: qualification records (ID, kind, version, verdict), version changes (first record per new version), advisories whose affected ranges include any version in use during the range, and a note when no advisory feed is available
- Rendering through the `periodic-review` template (`vap-template/1`)

**Out** (and which node owns it instead):
- Advisory feed content (owned by `QUAL.VULN.ADVIS`); fetching it (owned by `ECO.REL.UPD`)

## Requirements

- **REQ-QUAL.VAP.REVIEW-001** When `validate.make_review_report@1` runs for a date range, the system shall list every qualification record committed in that range with ID, kind, product version and verdict.
- **REQ-QUAL.VAP.REVIEW-002** The report shall list every product version change in the range, identified by the first record of each new version.
- **REQ-QUAL.VAP.REVIEW-003** The report shall list every advisory in the local feed whose affected versions include a version in use during the range.
- **REQ-QUAL.VAP.REVIEW-004** If no advisory feed is available locally, then the report shall say so in its advisory section rather than show an empty list.
- **REQ-QUAL.VAP.REVIEW-005** If the from date is after the to date, then the command shall fail with `validate.bad_range` and write nothing.

## Acceptance criteria

- **AC-1** Given fixture project `fixtures/qual/vap/review/two-versions.fid` (records for 1.3.0 and 1.4.0) and feed `fixtures/qual/vap/review/feed.json` (one advisory affecting 1.3.x), when `fid validate make-review-report --from 2027-01-01 --to 2027-06-30 --feed feed.json --root out --name review-h1 --json` runs, then the report lists 4 records, one version change and the advisory, and equals the golden file.
- **AC-2** Given the same without a feed, then the advisory section states that no advisory feed was available.
- **AC-3** (error path) Given `--from 2027-07-01 --to 2027-01-01`, then it fails with `validate.bad_range`.
- **AC-4** (MCP) Given AC-1 inputs as handles, when `validate_make_review_report` is called, then the result equals the CLI JSON.

## Design

- **Registry command(s):** `validate.make_review_report@1` (W1, `fs_effect: write_root`; params `{ from: date, to: date, feed?: import handle, root: handle, name: string }`; result `{ file_handle, digest, counts }`).
- **Data structures:** `ReviewReport { range, records, version_changes, advisories, feed_status: present|absent }`.
- **Algorithm:** filter records by commit time (UTC dates, inclusive); versions in use = versions of records in range plus the version in use at range start; semver range match against feed entries.
- **Errors:** `validate.bad_range`, `validate.feed_parse`, `validate.export_exists`.
- **Determinism:** D0.
- **Performance budget:** under 1 s.
- **Crate:** `fid-qual` (module `vap::review`).

## Test fixtures

`fixtures/qual/vap/review/` synthetic project and feed; CC-BY-4.0.

## Build prompt

> In crate `fid-qual`, add module `vap::review` and register `validate.make_review_report@1` (W1, fs_effect write_root; CLI `fid validate make-review-report --from --to [--feed <handle>] --root <handle> --name <n>`, MCP `validate_make_review_report`). Using `record` queries, list qualification records in the inclusive UTC date range, version changes (first record of each new version), and advisories from a locally supplied `qual-advisory-feed/1` file whose affected ranges include any version in use; state when no feed is present. Render with the `periodic-review` template (`vap::tpl`) to an approved export root; refuse inverted ranges and existing files. No network. No panics, deny unwrap/expect. Tests: AC-1..AC-4 of spec node QUAL.VAP.REVIEW.

## Open questions

- Whether `ECO.REL.UPD` keeps a local copy of advisory entries that this command can read without an import is cross-domain.

## Children

_Leaf: none._
