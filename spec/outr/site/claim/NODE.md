---
id: OUTR.SITE.CLAIM
title: Capability-claim check
level: 3
parent: OUTR.SITE
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-OUTR.SITE-003]
covers: []
depends_on: [ECO.PARITY]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.SITE.CLAIM: Capability-claim check

## Purpose

A build-time check that blocks publication of any website page that claims a capability the published release's parity report does not show as live. Pages declare their claims with explicit capability tags; the check joins those tags with the parity report and fails the site build on any unsupported claim. This is project tooling, not a product registry command, so it has no `fid` CLI, MCP tool or journal event.

## Scope

**In:** the capability-tag syntax for page sources; the check that reads page sources and a parity report and reports unsupported claims; its exit codes and JSON report.
**Out:** generating the parity report (`ECO.PARITY`); vocabulary and claim wording rules (`OUTR.BRAND.LINT`); running the check in the site pipeline (`OUTR.SITE.GEN` calls it); other quality gates (`OUTR.SITE.QA`).

## Requirements

- **REQ-OUTR.SITE.CLAIM-001** The check shall recognise a capability tag in Markdown sources as an HTML comment of the form `<!-- cap: <ID> -->` and in HTML sources as a `data-cap="<ID>"` attribute, where `<ID>` is a seed row ID such as `GOM.ALN.04`.
- **REQ-OUTR.SITE.CLAIM-002** If a tagged ID is marked not live in the parity report given to the check, then the check shall report an error naming the file, line, ID and the parity report's release.
- **REQ-OUTR.SITE.CLAIM-003** If a tagged ID does not appear in the parity report at all, then the check shall report an error of a distinct kind (unknown capability) with file, line and ID.
- **REQ-OUTR.SITE.CLAIM-004** The check shall exit with code 0 when there are no errors, 1 when there is at least one error, and 2 when an input cannot be read or parsed.
- **REQ-OUTR.SITE.CLAIM-005** The check shall write a JSON report whose findings are sorted by file path, then line, then ID, so that two runs on the same inputs produce byte-identical reports.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/claim/ok` (three pages tagging `GOM.ALN.04`, `PCD.MEAS.01` and `SA.NET.02`, and a parity file marking all three live), when `fid-outreach cap-check --parity parity.json --src pages --json` runs, then it exits 0 and the report has zero findings.
- **AC-2** Given fixture `fixtures/outr/claim/not-live` (as `ok`, but `SA.NET.02` marked not live), when the check runs, then it exits 1 with exactly one finding of kind `not_live` naming `pages/network.md`, its line number, `SA.NET.02` and the parity file's release tag.
- **AC-3** Given fixture `fixtures/outr/claim/unknown` (a page tagging `XX.YY.99`, absent from the parity file), when the check runs, then it exits 1 with one finding of kind `unknown_capability`.
- **AC-4** (error path) Given fixture `fixtures/outr/claim/bad-parity` (truncated JSON), when the check runs, then it exits 2, names the parity file, and writes no report.
- **AC-5** (determinism) Given fixture `not-live` with the page files listed in reverse directory order, when the check runs twice, then both JSON reports are byte-identical.
- **AC-6** (HTML) Given fixture `fixtures/outr/claim/html` with `<span data-cap="GOM.ALN.04">` in an HTML file and the ID marked not live, when the check runs, then it reports one `not_live` finding with the correct line.

## Design

- **Command:** `fid-outreach cap-check --parity <file> --src <dir> [--json] [--out <file>]`. Project tooling, not a registry command; nearest class R0 (reads files, writes only its own report).
- **Parity input contract:** the check reads only these fields from ECO.PARITY's `parity.json`: `release` (string) and `rows` (array of objects with `seed_id` string and `live` boolean). Any other fields are ignored. If ECO.PARITY names them differently, a one-line field mapping is added here.
- **Data structures:** `Tag { file, line, id }`, `Finding { kind: NotLive | UnknownCapability, file, line, id, release }`, `Report { release, findings }`.
- **Algorithm:** walk `--src` for `*.md` and `*.html` in sorted path order; scan each line with two bounded regular expressions (tag length capped at 64 characters); look each ID up in a sorted map built from the parity rows; emit findings; sort; serialise as canonical JSON (RFC 8785) for the report.
- **Errors:** `InputUnreadable(path)`, `ParityParse(path, reason)`, `TagMalformed(file, line)` (reported as a finding of kind `malformed_tag`, exit 1).
- **Determinism:** D0. No clock, no randomness, sorted traversal and canonical output.
- **Performance budget:** 1,000 pages and a 5,000-row parity file in under 1 s on a PLAT reference machine [unverified until measured].
- **Crate:** `fid-outreach` (outreach tooling workspace member), module `cap_check`.

## Test fixtures

Synthetic, hand-written fixtures under `fixtures/outr/claim/` (`ok`, `not-live`, `unknown`, `bad-parity`, `html`), licensed CC-BY-4.0 with the spec text (D-001). Seed IDs are illustrative strings; the check does not need the real seed inventories.

## Build prompt

> Build the `cap-check` subcommand of the `fid-outreach` Rust crate, an outreach tooling binary (not part of the product kernel or registry). Signature: `fid-outreach cap-check --parity <file> --src <dir> [--json] [--out <file>]`.
>
> Purpose: fail a static website build when a page claims a capability that the project's parity report does not show as live.
>
> Inputs: (1) a parity JSON file; read only `release` (string) and `rows[]` with `seed_id` (string) and `live` (bool); ignore all other fields. (2) A directory of `.md` and `.html` page sources. A capability tag is `<!-- cap: ID -->` in Markdown or a `data-cap="ID"` attribute in HTML; an ID is dotted uppercase like `GOM.ALN.04`, at most 64 characters.
>
> Behaviour: report `not_live` when a tagged ID has `live: false`; `unknown_capability` when the ID is not in `rows`; `malformed_tag` for a tag that does not parse. Each finding has kind, file (relative path), line (1-based), id and the parity `release`. Exit 0 with no findings, 1 with any finding, 2 if an input cannot be read or the parity JSON does not parse (write no report in that case). Sort files by path and findings by file, line, id; write the report as RFC 8785 canonical JSON so two runs on the same inputs are byte-identical.
>
> Acceptance tests (create these fixtures under `fixtures/outr/claim/`): `ok` exits 0 with zero findings; `not-live` exits 1 with one `not_live` finding for `SA.NET.02` in `pages/network.md`; `unknown` exits 1 with one `unknown_capability`; `bad-parity` (truncated JSON) exits 2 and writes no report; running `not-live` twice gives byte-identical reports; `html` finds a `data-cap` tag with the right line.
>
> Constraints: no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded line and tag lengths; fail closed on bad input; no network access; no std HashMap (use BTreeMap) so output order is deterministic; dependencies only under licences on the project allowlist (MIT OR Apache-2.0 compatible). Do not copy text from any other tool.

## Open questions

- The `parity.json` field names are assumed; ECO.PARITY is not yet expanded. Confirm or map the two fields when it is.

## Children

_Leaf: none._
