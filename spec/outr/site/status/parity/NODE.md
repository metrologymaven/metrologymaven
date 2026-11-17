---
id: OUTR.SITE.STATUS.PARITY
title: Per-release status page embedding parity and coverage reports
level: 4
parent: OUTR.SITE.STATUS
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.SITE.STATUS-001, REQ-OUTR.SITE.STATUS-002, REQ-OUTR.SITE.STATUS-005]
covers: []
depends_on: [OUTR.SITE.GEN.BUILD, OUTR.SITE.GEN.IMPORT]
uses_artifacts: [ECO.PARITY.GEN, ECO.PARITY.STATUS, ECO.COMM.SCOPE]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.SITE.STATUS.PARITY: Per-release status page embedding parity and coverage reports

## Purpose

Renders one status page per published release that embeds that release's generated reports (ECO parity `parity.md`, per-app `status.md`, and the spec tree's `COVERAGE.md` and `INDEX.md`) exactly as generated, shows where each came from, and keeps every earlier release browsable. The page adds navigation and provenance around the reports but never changes a byte of them, so a reader can check the page against the published files.

## Scope

**In:**
- The `status` renderer: `status/<tag>/index.html` per release with staged reports, `status/index.html` listing all releases, raw copies of each embedded report next to its page
- Byte-exact embedding and the extraction test that proves it
- The provenance line per report (source revision, release tag, generation date) from `build-manifest.json`
- A generated per-app summary table with stable anchors `app-<id>` (linked from app pages)

**Out:**
- Generating the reports (`ECO.PARITY.GEN`, `ECO.PARITY.STATUS`, `ECO.COMM.SCOPE`)
- Pinning and verifying the report files (`OUTR.SITE.GEN.IMPORT`)
- The roadmap page (`OUTR.SITE.STATUS.ROAD`)
- Per-app pages (`OUTR.SITE.PAGES.APP`)

## Requirements

- **REQ-OUTR.SITE.STATUS.PARITY-001** For each release that has a staged `parity.md`, the renderer shall write `status/<tag>/index.html` embedding `parity.md`, and `status.md`, `COVERAGE.md` and `INDEX.md` where they are staged for that release, each in its own `<pre>` element whose text content, after HTML entity decoding, equals the staged file byte-for-byte.
- **REQ-OUTR.SITE.STATUS.PARITY-002** The renderer shall copy each embedded report unchanged to `status/<tag>/<file name>` and link it from the page.
- **REQ-OUTR.SITE.STATUS.PARITY-003** Next to each embedded report, the page shall show the report's release tag, source revision and date exactly as recorded for that input in `build-manifest.json`, and a report without a source revision shall show "source revision not recorded".
- **REQ-OUTR.SITE.STATUS.PARITY-004** When `status.json` is staged for a release, the page shall show, above the embedded reports, a table with one row per app (status, live and total) whose row element carries `id="app-<id>"`.
- **REQ-OUTR.SITE.STATUS.PARITY-005** The renderer shall write `status/index.html` listing every release with a status page, newest first by release date then tag, and shall never drop a release whose reports are still staged.
- **REQ-OUTR.SITE.STATUS.PARITY-006** If a release's status page would embed a report whose name is not one of `parity.md`, `status.md`, `COVERAGE.md` or `INDEX.md`, or a staged report is larger than 8 MiB, then the build shall fail and name the file.
- **REQ-OUTR.SITE.STATUS.PARITY-007** While no release has a staged `parity.md`, `status/index.html` shall state "No release has been published yet" and link the current spec-tree `COVERAGE.md` if one is staged.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/site/status/parity/two-releases` (staged reports for `v0.1.0` and `v0.2.0`, including Markdown with `<`, `&` and non-ASCII characters), when `fid-outreach site build` runs, then for every embedded report the test extracts the `<pre>` text, decodes entities and compares it byte-for-byte with the staged file, and all match.
- **AC-2** (raw copies) Given the AC-1 output, then `status/v0.1.0/parity.md` is byte-identical to the staged file and the page links it.
- **AC-3** (provenance) Given the AC-1 output, then each report on `status/v0.2.0/index.html` shows the tag, source revision and date from the fixture's `build-manifest.json`.
- **AC-4** (archive) Given the AC-1 output, then `status/index.html` lists `v0.2.0` before `v0.1.0` and both pages exist.
- **AC-5** (anchors) Given a staged `status.json` with apps inspect and report, then `status/v0.2.0/index.html` contains elements with `id="app-inspect"` and `id="app-report"`.
- **AC-6** (oversize) Given fixture `oversize` (a 9 MiB `parity.md`), when the build runs, then it exits 1 with `STATUS-SIZE parity.md`.
- **AC-7** (no release) Given fixture `none` (no release reports, one spec-tree `COVERAGE.md`), then `status/index.html` contains "No release has been published yet" and a link to the coverage report.
- **AC-8** (determinism) Building `two-releases` twice gives byte-identical status pages.

## Design

- **Command:** none of its own; the renderer `status` runs inside `fid-outreach site build` and registers itself in `site::renderers::all()`. Nearest class W1.
- **Embedding:** escape only `&`, `<` and `>` (and `"` is not needed inside element text); write the file content between `<pre class="report" data-file="<name>">` and `</pre>` with no added leading newline (HTML drops one leading newline after `<pre>`, so the renderer inserts one newline after the opening tag on purpose and the test accounts for that by the HTML parsing rule).
- **Provenance:** look up each embedded file's path in the staged `build-manifest.json`; show `release`, `source_rev`, `date`.
- **Status-report contract:** as OUTR.SITE.PAGES.APP: reads only `apps[]` `id`, `status`, `live`, `total` [unverified schema of ECO.PARITY.STATUS].
- **Errors:** `STATUS-NAME(file)`, `STATUS-SIZE(file)`, `STATUS-PROVENANCE(file)` (input not in build manifest).
- **Determinism:** D0.
- **Performance budget:** 20 releases with 2 MB of reports each in under 2 s [unverified].
- **Crate:** `fid-outreach`, module `site::status::parity`.

## Test fixtures

Synthetic fixtures under `fixtures/outr/site/status/parity/` (`two-releases`, `oversize`, `none`) with reports containing HTML-special and non-ASCII characters, and a staged `build-manifest.json`; CC-BY-4.0. An HTML5-conformant parser in the test extracts `<pre>` text.

## Build prompt

> In the `fid-outreach` Rust crate (outreach tooling), add the renderer `status` (module `site::status::parity`) implementing the existing `Renderer` trait and register it in `site::renderers::all()`.
>
> For each release tag with a staged `<tag>/parity.md`, write `status/<tag>/index.html` that embeds `parity.md`, and `status.md`, `COVERAGE.md`, `INDEX.md` when staged for that tag, each in `<pre class="report" data-file="...">` with only `&`, `<`, `>` escaped, so that text extracted by an HTML5 parser equals the file byte-for-byte (insert one newline after `<pre>` because HTML drops the first one). Copy each report unchanged to `status/<tag>/<name>` and link it. Beside each report show release, source revision and date from the staged `build-manifest.json` ("source revision not recorded" when absent; fail STATUS-PROVENANCE if the file is not in the manifest). If `status.json` is staged, add a table with one row per app (status, live, total) whose row has `id="app-<id>"`. Write `status/index.html` listing all releases newest first (release date, then tag); with no releases, say "No release has been published yet" and link a staged spec-tree COVERAGE.md if present. Fail STATUS-NAME for other report names and STATUS-SIZE above 8 MiB.
>
> Tests: AC-1..AC-8 of spec node OUTR.SITE.STATUS.PARITY, including a byte-for-byte extraction test with an HTML5 parser on reports containing `<`, `&` and non-ASCII text. Constraints: no panics, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; deterministic output; never modify report bytes.

## Open questions

- Rendering the Markdown reports as formatted tables would read better but breaks the byte-for-byte rule; the raw `<pre>` view is the record, and a formatted view could be added later as a second, clearly labelled block.

## Children

_Leaf: none._
