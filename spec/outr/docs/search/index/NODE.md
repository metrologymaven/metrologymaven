---
id: OUTR.DOCS.SEARCH.INDEX
title: Per-edition search index with command-name table
level: 4
parent: OUTR.DOCS.SEARCH
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.DOCS.SEARCH-001, REQ-OUTR.DOCS.SEARCH-003, REQ-OUTR.DOCS.SEARCH-004, REQ-OUTR.DOCS.SEARCH-005]
covers: []
depends_on: [OUTR.DOCS.FRAME.NAV]
uses_artifacts: [OUTR.DOCS.REF]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.DOCS.SEARCH.INDEX: Per-edition search index with command-name table

## Purpose

Builds, at docs build time, one search index per release edition from that edition's built pages, plus an exact-name table that maps every command ID, CLI path and MCP tool name of the edition to its reference pages. The index is written as a script file so that the browser can load it from local files in the offline bundle, and its size is held to a recorded budget.

## Scope

**In:** the `search-index` subcommand; the index format; tokenisation; the exact-name table; the size budget file and check.
**Out:** the in-browser search code and page (`OUTR.DOCS.SEARCH.UI`); page content (`OUTR.DOCS.REF`, `OUTR.DOCS.BOOK`, `OUTR.DOCS.CONTRIB`, `OUTR.CONTENT`); the edition build (`OUTR.DOCS.FRAME.NAV`).

## Requirements

- **REQ-OUTR.DOCS.SEARCH.INDEX-001** When `search-index` runs on an edition, it shall write `search-index.js` at the edition root containing an index of every HTML page of that edition only (title, relative URL, headings, and body terms).
- **REQ-OUTR.DOCS.SEARCH.INDEX-002** The index file shall be a JavaScript file that assigns one data object to a single global name, so it loads with a relative `<script>` element from `file://` without any fetch.
- **REQ-OUTR.DOCS.SEARCH.INDEX-003** The index shall include an exact-name table mapping each command ID (with and without `@major`), CLI path (`fid <domain> <verb-object>`) and MCP tool name of the edition's reference pages to the URLs of that command's CLI, MCP and Python pages.
- **REQ-OUTR.DOCS.SEARCH.INDEX-004** If the index file exceeds the byte budget in `search-budget.toml` (initial 5 MB [unverified]), then the tool shall exit 1 stating the size and the budget.
- **REQ-OUTR.DOCS.SEARCH.INDEX-005** Two runs on the same edition shall produce byte-identical index files.

## Acceptance criteria

- **AC-1** Given the edition built from fixture `fixtures/outr/docs/search/edition` (ten book pages and reference pages for `align.run_bestfit@1` and `registry.show_manifest@1`), when `fid-outreach search-index --edition site/v0.2.0 --budget search-budget.toml` runs, then `site/v0.2.0/search-index.js` exists and parses as a single global assignment.
- **AC-2** Given the AC-1 index, when a test looks up `align.run_bestfit`, `align.run_bestfit@1`, `fid align run-bestfit` and `align_run_bestfit` in the exact-name table, then each maps to the three reference page URLs of that command.
- **AC-3** Given the AC-1 index, when a test lists indexed URLs, then all are relative and inside `v0.2.0/`.
- **AC-4** (budget) Given a budget of 1 KB, when the tool runs, then it exits 1 with the size and the budget.
- **AC-5** (determinism) Given AC-1 run twice, then the index files are byte-identical.

## Design

- **Command:** `fid-outreach search-index --edition <dir> --budget <file> [--json]`. Project tooling; nearest class W1 (writes one file into the local edition).
- **Format:** `window.FID_SEARCH_INDEX = {"v":1,"docs":[{"t":title,"u":url,"h":[headings]}],"terms":{token:[[doc,weight]]},"names":{name:[urls]}};` with keys sorted, compact JSON.
- **Tokenisation:** lower-case; split on non-alphanumerics except `.`, `_`, `@` and `-` inside identifiers; drop tokens shorter than 2 characters; weights 3 for title, 2 for headings, 1 for body; no stemming.
- **Names:** command IDs come from reference page paths `ref/<surface>/<command_id>@<major>.html`; CLI and MCP forms are derived by CONSTITUTION §3.
- **Errors:** `EditionUnreadable`, `BudgetParse`, finding `over_budget`.
- **Determinism:** D0.
- **Performance budget:** a 2,000-page edition indexed in under 10 s [unverified until measured].
- **Crate:** `fid-outreach`, module `search_index`.

## Test fixtures

A synthetic edition under `fixtures/outr/docs/search/edition` built with `docs-build` and `ref-render` from their fixtures. CC-BY-4.0.

## Build prompt

> Build `fid-outreach search-index --edition <dir> --budget <file> [--json]` in the Rust outreach tooling crate `fid-outreach`. Read every HTML page of the edition (bounded HTML tokenizer; skip `script`, `style`, `nav`); collect title, relative URL, headings and body tokens (lower-case; split on non-alphanumerics but keep `.`, `_`, `@`, `-` inside identifiers; drop tokens under 2 characters; weights title 3, heading 2, body 1). Build an exact-name table from reference page paths `ref/<cli|mcp|python>/<command_id>@<major>.html`: keys `command_id`, `command_id@major`, `fid <domain> <verb-object>` (underscores to hyphens in the verb-object) and the MCP name (dots to underscores), each mapping to the command's three page URLs. Write `search-index.js` at the edition root as `window.FID_SEARCH_INDEX = <compact JSON with sorted keys>;`. If its size exceeds `max_bytes` in the budget TOML, exit 1 with size and budget. Two runs must be byte-identical.
>
> Acceptance tests under `fixtures/outr/docs/search/` as listed in the spec.
>
> Constraints: no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded reads; no network; BTreeMap only; allowlisted dependency licences.

## Open questions

- If the budget is hit, options are dropping body terms of reference pages (they are reachable through the name table) or splitting the index per section; maintainers choose when it happens.

## Children

_Leaf: none._
