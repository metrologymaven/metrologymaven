---
id: OUTR.SITE.QA.CRAWL
title: Headless-browser crawl and off-origin request gate
level: 4
parent: OUTR.SITE.QA
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.SITE.QA-001, REQ-OUTR.SITE.QA-002]
covers: []
depends_on: [OUTR.SITE.GEN.HOST]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.SITE.QA.CRAWL: Headless-browser crawl and off-origin request gate

## Purpose

Loads every HTML page of a built static site in a pinned headless browser served from a loopback file server, records every network request each page makes, blocks and reports every request to another origin, and writes a crawl log that the size gate reuses. It proves the no-third-party-request rule on what a browser actually does, not on what a static scan guesses, and it never lets a blocked request leave the machine.

## Scope

**In:**
- `fid-outreach qa crawl`: serve the build output on `127.0.0.1`, drive a headless Chromium-family browser over the DevTools protocol, visit each page, scroll to the end, wait for network idle
- Interception: every request to an origin other than the serving origin (or the configured public origin) is failed before it is sent and recorded
- Crawl log `crawl.json`: per page, every request with URL, resource type, own-origin flag, body size and initiator
- The pinned browser version in the site repository

**Out:**
- The loopback file server (`OUTR.SITE.GEN.HOST`, reused)
- Accessibility checks (`OUTR.SITE.QA.A11Y`)
- The size budget computed from the crawl log, lint, configuration and combined report (`OUTR.SITE.QA.RUN`)

## Requirements

- **REQ-OUTR.SITE.QA.CRAWL-001** When the crawl runs, it shall load every `.html` file under the site root (or every page in the configured page list) in a headless browser at its URL on a loopback server, scroll to the end of the page, and wait until no request has been in flight for 500 ms or 10 s have passed.
- **REQ-OUTR.SITE.QA.CRAWL-002** The crawl shall intercept every request whose origin is neither the loopback serving origin nor the configured public origin, fail it before any byte is sent, and record it as off-origin.
- **REQ-OUTR.SITE.QA.CRAWL-003** If any page made at least one off-origin request, then the crawl shall exit 1.
- **REQ-OUTR.SITE.QA.CRAWL-004** For each failing page, the report shall list every off-origin URL with the initiator type and, where the browser reports them, the initiating document or script URL and line number.
- **REQ-OUTR.SITE.QA.CRAWL-005** The crawl log shall record, for every request of every page, the URL, resource type, own-origin flag and the response body size in bytes for own-origin responses, with pages sorted by path and requests by URL.
- **REQ-OUTR.SITE.QA.CRAWL-006** The site repository shall pin the browser build used by the crawl, and the crawl shall exit 2 if the installed browser's version differs from the pinned one.
- **REQ-OUTR.SITE.QA.CRAWL-007** If a page fails to load or times out after 30 s, then the crawl shall exit 2 and name the page.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/site/qa/crawl/clean` (five pages with self-hosted CSS, font and image), when `fid-outreach qa crawl --root clean --out crawl.json` runs, then it exits 0, `crawl.json` lists five pages, and every request is own-origin with a body size.
- **AC-2** Given fixture `font-cdn` (one page with `<link rel="stylesheet" href="https://fonts.example.com/css">`), when the crawl runs, then it exits 1, the report names that page and URL with initiator type `parser` and line number, and the loopback test proxy confirms no request reached the network.
- **AC-3** Given fixture `script-beacon` (a same-origin script that calls `fetch("https://stats.example.net/hit")` after load), when the crawl runs, then it exits 1 and the finding names the script URL as initiator.
- **AC-4** Given fixture `lazy-image` (an off-origin image with `loading="lazy"` at the bottom of a long page), when the crawl runs, then it is detected because of the scroll step.
- **AC-5** (public origin) Given fixture `public-origin` (a request to the configured public origin) and `--public-origin https://site.example`, then the request is recorded as own-origin and the crawl exits 0.
- **AC-6** (pin) Given a pinned version different from the installed browser, then the crawl exits 2 with `CRAWL-BROWSER`.
- **AC-7** (stable log) Running AC-1 twice gives byte-identical `crawl.json` files: page and request lists, types and sizes match.

## Design

- **Command:** `fid-outreach qa crawl --root <dir> --out <file> [--pages <list file>] [--public-origin <origin>] [--browser <path>]`. Project tooling; nearest class R0 (reads files, drives a local browser, binds loopback, writes only its report). No request leaves the machine because off-origin requests are failed at interception.
- **Browser control:** DevTools protocol over a pipe or loopback socket; `Fetch.enable` with a catch-all pattern; `Fetch.requestPaused` fails off-origin requests with `BlockedByClient` and continues own-origin ones; `Network.requestWillBeSent` gives the initiator; `Network.loadingFinished` gives encoded sizes. Rust DevTools client crate under the project licence allowlist [unverified which crate qualifies]. Browser pin in `qa-browser.toml` (`product`, `version`).
- **Isolation:** fresh browser profile per run, service workers disabled, cache disabled, one tab reused page by page.
- **Data structures:** `crawl.json`: `{ browser_version, pages: [{ path, status, requests: [{ url, type, own_origin, bytes?, initiator: { type, url?, line? } }] }] }`, canonical JSON.
- **Errors:** `CRAWL-OFF-ORIGIN(page, url)` (exit 1), `CRAWL-BROWSER(expected, found)`, `CRAWL-LOAD(page)`, `CRAWL-IO` (exit 2).
- **Determinism:** N: request timing depends on the browser. The log content is stable for static pages because it is sorted and holds no timings.
- **Performance budget:** 200 pages in under 120 s on the CI runner [unverified until measured].
- **Crate:** `fid-outreach`, module `qa::crawl`; reuses the loopback server of `site::host`.

## Test fixtures

Synthetic sites under `fixtures/outr/site/qa/crawl/` (`clean`, `font-cdn`, `script-beacon`, `lazy-image`, `public-origin`) whose off-origin URLs use reserved example domains; a loopback catch-all proxy in the test asserts nothing was forwarded. CC-BY-4.0. CI installs the pinned browser build.

## Build prompt

> In the `fid-outreach` Rust crate (outreach tooling), add `qa crawl --root <dir> --out <file> [--pages <file>] [--public-origin <origin>] [--browser <path>]` (module `qa::crawl`).
>
> Serve `<root>` with the existing loopback file server from `site::host`. Start the pinned headless Chromium-family browser (version from `qa-browser.toml`; exit 2 with CRAWL-BROWSER on mismatch) with a fresh profile, cache and service workers disabled. Over the DevTools protocol enable Fetch interception for all URLs: fail any request whose origin is neither the loopback origin nor `--public-origin` (record it as off-origin, never send it) and continue the rest; use Network events for initiator (type, url, line) and encoded body size. For each page (all `.html` under root, sorted, or the `--pages` list): navigate, wait for load, scroll to the end, wait for 500 ms without in-flight requests (cap 10 s); a load failure or 30 s timeout exits 2 with CRAWL-LOAD.
>
> Write `crawl.json` (canonical JSON): browser_version and pages sorted by path, each with requests sorted by URL (url, type, own_origin, bytes for own-origin, initiator). Exit 1 if any off-origin request was recorded, listing per page each URL and initiator; exit 0 otherwise.
>
> Tests: AC-1..AC-7 of spec node OUTR.SITE.QA.CRAWL with fixtures under `fixtures/outr/site/qa/crawl/`; use reserved example domains and a loopback proxy to prove nothing leaves the machine. Constraints: no panics, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded timeouts; dependencies under the project SPDX allowlist (MIT OR Apache-2.0 compatible, no MPL or GPL family).

## Open questions

- Which Rust DevTools-protocol client meets the licence allowlist is unverified; if none does, a small purpose-built client over the browser's pipe transport fits in the size budget.

## Children

_Leaf: none._
