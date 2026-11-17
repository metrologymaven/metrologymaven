---
id: OUTR.SITE.GEN.HOST
title: Self-hosting guide and generic static-server test
level: 4
parent: OUTR.SITE.GEN
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-OUTR.SITE.GEN-006]
covers: []
depends_on: [OUTR.SITE.GEN.BUILD]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.SITE.GEN.HOST: Self-hosting guide and generic static-server test

## Purpose

Lets anyone mirror the project website: a short guide in the site repository that explains how to build the site and serve the output folder from any static web server, plus an automated test that proves the claim by serving the built output from a generic file server under a path prefix with no rewrite rules, and following every internal link.

## Scope

**In:**
- `SELF-HOSTING.md` in the site repository: build command, output folder, serving with any static server, path prefixes, recommended (optional) headers, how to verify a mirror against `build-manifest.json`
- `fid-outreach site serve-check`: serves a built output folder from a minimal loopback file server under a configurable prefix and resolves every internal link and asset reference

**Out:**
- Making links relative and output static (`OUTR.SITE.GEN.BUILD`)
- Off-origin request checks in a browser (`OUTR.SITE.QA.CRAWL`)
- Hosting the project's own public instance (a maintainer operation, not specified here)

## Requirements

- **REQ-OUTR.SITE.GEN.HOST-001** The site repository shall contain `SELF-HOSTING.md` that names the build command, the output folder, the requirement that the server serves `index.html` for a directory request, and the steps to compare a mirror's `build-manifest.json` with the project's.
- **REQ-OUTR.SITE.GEN.HOST-002** The guide shall state that no server-side code, rewrite rule, database or third-party service is needed, and shall list any HTTP header it recommends as optional.
- **REQ-OUTR.SITE.GEN.HOST-003** When `serve-check` serves a built output folder under a path prefix from a server that maps URLs directly to files and serves `index.html` for directories, every internal `href`, `src` and CSS `url()` in every HTML and CSS file shall resolve to an existing file with HTTP status 200.
- **REQ-OUTR.SITE.GEN.HOST-004** If any internal reference does not resolve, then `serve-check` shall exit 1 and list the source file, the reference and the status for each failure, sorted by source file then reference.
- **REQ-OUTR.SITE.GEN.HOST-005** The site's CI shall run `serve-check` on every build with the prefixes `/` and `/mirror/site/`.

## Acceptance criteria

- **AC-1** Given the golden output of fixture `fixtures/outr/site/build/minimal`, when `fid-outreach site serve-check --root expected --prefix /mirror/site/` runs, then it exits 0 and reports the number of references checked.
- **AC-2** (error path) Given fixture `fixtures/outr/site/host/broken` (a page linking `../missing/index.html` and a stylesheet with `url(fonts/x.woff2)` that does not exist), when serve-check runs, then it exits 1 and lists exactly those two references with status 404, in sorted order.
- **AC-3** (absolute link) Given fixture `fixtures/outr/site/host/absolute` (a page with `href="/apps/"`), when serve-check runs with prefix `/mirror/site/`, then it exits 1 naming that reference.
- **AC-4** (guide) Given the site repository, when the guide test reads `SELF-HOSTING.md`, then it finds the build command string used in CI, the words `index.html`, `build-manifest.json` and a section titled "Optional headers".
- **AC-5** (determinism) Running AC-2 twice gives byte-identical output.

## Design

- **Command:** `fid-outreach site serve-check --root <dir> --prefix <path> [--json]`. Project tooling; nearest class R0 (reads files, binds a loopback port, writes only its report to stdout).
- **Server:** an in-process HTTP/1.1 file server bound to `127.0.0.1` on an ephemeral port; maps `<prefix><path>` to `<root>/<path>`; serves `index.html` for directory paths; no rewrites, no fallbacks; rejects `..` segments.
- **Algorithm:** parse every `.html` file for `href`, `src`, `srcset` and inline `style` URLs, and every `.css` file for `url()` and `@import`; skip references with a scheme or `//` (left to the crawl gate) and `#fragment`-only links; resolve relative to the page URL under the prefix; request each unique URL once; collect failures; sort.
- **Errors:** `SERVE-UNRESOLVED(file, ref, status)`, `SERVE-IO`.
- **Determinism:** D0 for the report (sorted, no timing data).
- **Performance budget:** 500 pages in under 5 s [unverified until measured].
- **Crate:** `fid-outreach`, module `site::host`. The guide is a Markdown file at the site repository root.

## Test fixtures

Reuses the golden output of `fixtures/outr/site/build/minimal`; adds `fixtures/outr/site/host/broken` and `absolute`. Synthetic, CC-BY-4.0.

## Build prompt

> In the `fid-outreach` Rust crate (outreach tooling), add `site serve-check --root <dir> --prefix <path> [--json]` and write `SELF-HOSTING.md` at the site repository root.
>
> serve-check: start an in-process HTTP/1.1 file server on 127.0.0.1 (ephemeral port) that maps `<prefix><path>` to files under `<root>`, serves `index.html` for directories, has no rewrites and rejects `..`. Parse every HTML file for href, src, srcset and inline style URLs and every CSS file for url() and @import; skip scheme, protocol-relative and fragment-only references; resolve the rest relative to the page under the prefix; request each unique URL once; exit 1 listing every non-200 reference (source file, reference, status) sorted by file then reference; exit 0 otherwise. Add a CI step that runs it with prefixes `/` and `/mirror/site/`.
>
> SELF-HOSTING.md: the exact build command used in CI, the output folder, that any static server works if it serves index.html for directories, that no server-side code, rewrite rule, database or third-party service is needed, a section "Optional headers" (for example a content-security policy allowing only the same origin), and how to compare a mirror's build-manifest.json with the project's.
>
> Tests: AC-1..AC-5 of spec node OUTR.SITE.GEN.HOST. Constraints: no panics, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; loopback only; bounded request and file sizes; dependencies under the project SPDX allowlist.

## Open questions

- None.

## Children

_Leaf: none._
