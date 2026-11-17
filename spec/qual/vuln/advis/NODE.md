---
id: QUAL.VULN.ADVIS
title: Advisory format, pages and feed
level: 3
parent: QUAL.VULN
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-QUAL.VULN-005, REQ-QUAL.VULN-009]
covers: []
depends_on: [QUAL.VULN.TRIAGE]
uses_artifacts: [ECO.REL.UPD, ECO.REL.SIGN.CUSTODY]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# QUAL.VULN.ADVIS: Advisory format, pages and feed

## Purpose

Turns a fixed, confirmed vulnerability record into a published advisory: a machine-readable OSV-format document [unverified version], a human Markdown page and an entry in the advisory feed that the in-product update check (`ECO.REL.UPD`) and the periodic-review report (`QUAL.VAP.REVIEW`) read. When a flaw affects values of record, the advisory names the affected algorithm IDs and says whether past results need re-evaluation.

## Scope

**In:**
- Advisory generator from a record in state `fixed` plus a public summary file written for publication
- Machine-readable advisory (OSV JSON, with CVE alias when assigned) and Markdown page
- Advisory feed `advisories/feed.json` schema `qual-advisory-feed/1`: ID, aliases, published time, severity, affected and fixed versions, affected algorithm IDs, re-evaluation flag, page URL path
- Values-of-record section rule
- Check that published advisories contain no field marked private in the record

**Out** (and which node owns it instead):
- Record content and triage (owned by `QUAL.VULN.TRIAGE`)
- Showing advisories in the product (owned by `ECO.REL.UPD`)
- Signing the feed (release statements are signed by `ECO.REL.SIGN.CUSTODY`; see Open questions)
- CVE assignment route (maintainer choice; see Open questions)

## Requirements

- **REQ-QUAL.VULN.ADVIS-001** When the generator runs on a record in state `fixed` with a public summary, the tool shall write an OSV JSON advisory and a Markdown page holding the identifier and aliases, affected and fixed versions, severity system and score, workaround or the text "none known", and credit.
- **REQ-QUAL.VULN.ADVIS-002** If the record's state is not `fixed`, or its embargo end is later than the given publication time, then the generator shall refuse and write nothing.
- **REQ-QUAL.VULN.ADVIS-003** If the record has a values-of-record impact, then the advisory and page shall list every affected algorithm ID and state `reevaluate: true` or `false`, and the generator shall refuse a record whose impact omits either.
- **REQ-QUAL.VULN.ADVIS-004** The tool shall regenerate `advisories/feed.json` from all published advisories, sorted by published time then ID, as RFC 8785 canonical JSON.
- **REQ-QUAL.VULN.ADVIS-005** The generated advisory and page shall contain no text from the record's private fields (vector detail, reproduction notes, reporter contact, codewords), checked by exact substring search.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/qual/vuln/advis/fixed/` (record `FID-2027-0003` fixed in 1.4.2, public summary, publication time 2027-04-02T00:00Z), when `cargo xtask qual advisory publish --record FID-2027-0003.toml --summary summary.toml --at 2027-04-02T00:00:00Z --out out/` runs, then `out/advisories/FID-2027-0003.osv.json`, the page and `feed.json` equal the golden files.
- **AC-2** (error path) Given the same record with `embargo_until = 2027-04-10`, then the tool exits 1 and writes nothing.
- **AC-3** Given `vor/` (impact on `fit.circle.lsq@1.2.0`, `reevaluate = true`), then the page lists the algorithm ID and states re-evaluation is needed; given the impact without `reevaluate`, the tool refuses.
- **AC-4** (error path) Given `leak/` (summary text contains the reproduction note), then the tool exits 1 naming the field.
- **AC-5** (joint) Given AC-1's `feed.json`, when the `ECO.REL.UPD` advisory fixture reader parses it, then it reports advisory `FID-2027-0003` for installed version 1.4.1 only.

## Design

- **Registry command(s):** none; repository tool `cargo xtask qual advisory publish --record <toml> --summary <toml> --at <rfc3339> --out <dir>` (W1, local output root). Run by a maintainer when the coordinated publication date is reached.
- **Data structures:** `PublicSummary { title, description: Untrusted<String>, workaround, credit }`; `FeedEntry { id, aliases, published, severity, affected, fixed, algorithm_ids, reevaluate, page }`.
- **Algorithm:** validate state and embargo; map to OSV fields; render Markdown from a fixed template; rebuild the feed from every advisory file in the output tree.
- **Errors:** `ADV-STATE`, `ADV-EMBARGO`, `ADV-VOR-INCOMPLETE`, `ADV-LEAK(field)`.
- **Determinism:** D0.
- **Performance budget:** under 1 s.
- **Crate:** `fid-xtask` (module `qual::vuln::advis`).

## Test fixtures

`fixtures/qual/vuln/advis/{fixed,vor,leak}/` with synthetic records and summaries; CC-BY-4.0.

## Build prompt

> In `fid-xtask`, add module `qual::vuln::advis` with `qual advisory publish --record <toml> --summary <toml> --at <rfc3339> --out <dir>`, using the record types of `qual::vuln::triage`. Refuse unless the record is `fixed` and its embargo has ended at `--at`. Write an OSV JSON advisory (id, aliases incl. CVE when present, affected and fixed versions, severity, references) and a Markdown page from a fixed template (identifier, versions, severity, workaround or "none known", credit, and when values of record are affected: algorithm IDs and whether past results must be re-evaluated; refuse if either is missing). Refuse if any private record field text appears in the outputs. Rebuild `advisories/feed.json` (schema `qual-advisory-feed/1`, canonical JSON, sorted by published then id). No clock, no network; no panics, deny unwrap/expect. Tests: AC-1..AC-5 of spec node QUAL.VULN.ADVIS.

## Open questions

- Becoming a CVE Numbering Authority versus requesting IDs through a platform is a maintainer choice; the schema holds the CVE as an alias either way.
- `ECO.REL.UPD` reads advisory IDs from the channel feed; whether the channel feed embeds `feed.json` entries or links them needs agreement with `ECO.REL.PROC.CHANNEL` (cross-domain).
- Whether `feed.json` needs its own DSSE signature by `ECO.REL.SIGN.CUSTODY` is open (cross-domain).
- Bug bounties are out of scope unless funded.

## Children

_Leaf: none._
