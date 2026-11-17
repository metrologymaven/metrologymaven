---
id: OUTR.DOCS.SEARCH.UI
title: In-browser search page with no network requests
level: 4
parent: OUTR.DOCS.SEARCH
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-OUTR.DOCS.SEARCH-001, REQ-OUTR.DOCS.SEARCH-002, REQ-OUTR.DOCS.SEARCH-003, REQ-OUTR.DOCS.SEARCH-004]
covers: []
depends_on: [OUTR.DOCS.SEARCH.INDEX, OUTR.DOCS.FRAME.NAV]
uses_artifacts: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.DOCS.SEARCH.UI: In-browser search page with no network requests

## Purpose

The search box in the docs frame and the search results page: a small, dependency-free script that reads the edition's own index from a relative script file, ranks exact command-name matches first, then term matches, and makes no network request of any kind. It works the same on the hosted portal and from the unpacked offline bundle.

## Scope

**In:** `search.html`, `search.js`, `search.css` and the frame partial with the search form, shipped as assets of the outreach crate and copied into each edition by `docs-build`.
**Out:** the index and name table (`OUTR.DOCS.SEARCH.INDEX`); the off-origin crawl that proves zero third-party requests on every page (`OUTR.DOCS.QA`); the frame (`OUTR.DOCS.FRAME.NAV`).

## Requirements

- **REQ-OUTR.DOCS.SEARCH.UI-001** The search page shall load only the edition's own `search-index.js` through a relative `<script>` element and shall make no `fetch`, XMLHttpRequest, WebSocket or other network call.
- **REQ-OUTR.DOCS.SEARCH.UI-002** When the query, trimmed, equals a key of the index's exact-name table, the results shall list that command's reference pages first, in the order CLI, MCP, Python.
- **REQ-OUTR.DOCS.SEARCH.UI-003** For other queries, results shall be ranked by the summed term weights of the query's tokens, ties broken by URL, and show at most 50 results.
- **REQ-OUTR.DOCS.SEARCH.UI-004** The search page shall take the query from the `q` URL parameter, so the frame's search form works from `file://` and from the hosted portal.
- **REQ-OUTR.DOCS.SEARCH.UI-005** The search page shall meet the VIZ accessibility baseline: labelled input, results announced in a live region, full keyboard use.

## Acceptance criteria

- **AC-1** Given the fixture edition from OUTR.DOCS.SEARCH.INDEX served from a local origin, when a headless browser opens `search.html?q=fid%20align%20run-bestfit` and records network requests, then the first three results are the CLI, MCP and Python pages of `align.run_bestfit@1`, and every request is to the same origin.
- **AC-2** Given the unpacked offline bundle opened via `file://` with networking disabled, when the browser opens `search.html?q=journal`, then results appear and no request fails.
- **AC-3** Given `search.js`, when a test scans its source, then it contains none of `fetch(`, `XMLHttpRequest`, `WebSocket`, `import(`, `navigator.sendBeacon`.
- **AC-4** Given the search page, when the OUTR.SITE.QA accessibility gate runs on it, then it reports zero violations at the baseline's required level.

## Design

- **Registry command(s):** none; static assets.
- **Language:** plain JavaScript (ES2017), no framework, no build step, no third-party code; about 200 lines.
- **Ranking:** exact-name lookup first; then tokenise the query with the index rules, sum weights per document, sort by score descending then URL ascending.
- **Files:** `assets/search/search.html`, `search.js`, `search.css`, `search-box.html` (frame partial: a GET form to `search.html` with a labelled input `q`).
- **Determinism:** D0 (same index and query give the same result list).
- **Crate:** `fid-outreach` (assets folder `assets/search/`, embedded and copied by `docs-build`).

## Test fixtures

The OUTR.DOCS.SEARCH.INDEX fixture edition and the FRAME.BUNDLE archive of it. CC-BY-4.0.

## Build prompt

> Write the docs search assets for the outreach tooling crate `fid-outreach`, folder `assets/search/`: `search.html` (a page with a labelled search input, a results list in an `aria-live="polite"` region, and `<script src="search-index.js">` then `<script src="search.js">`, both relative), `search.js` (plain ES2017, no dependencies, no build step), `search.css`, and `search-box.html` (a GET form to `search.html` with input `q`, for the frame header). `search.js` reads `window.FID_SEARCH_INDEX` (`docs[] {t,u,h}`, `terms {token: [[doc, weight]]}`, `names {name: [urls]}`), takes the query from `location.search` parameter `q`, shows exact-name matches first (CLI, MCP, Python page order), then term matches ranked by summed weight, ties by URL, at most 50. It must make no network call: no `fetch`, `XMLHttpRequest`, `WebSocket`, dynamic `import`, or `sendBeacon`. Make `docs-build` copy these files into every edition and include `search-box.html` in the frame header.
>
> Acceptance tests: headless browser on a local origin and on `file://` with networking disabled (exact-name ranking, results present, no off-origin or failed request); a source scan for forbidden APIs; the accessibility gate passes.
>
> Constraints: no third-party scripts or fonts; escape result titles when inserting into the page (use `textContent`); keep the files small and readable.

## Open questions

- CONSTITUTION §2 lists Rust, WASM, Python and TypeScript (MCP shim only) for the product; it does not mention scripts in static documentation pages. This leaf uses a small plain-JavaScript file because client-side search needs one; a reviewer should confirm this reading, or choose a Rust-to-WASM search core embedded in a script file instead.

## Children

_Leaf: none._
