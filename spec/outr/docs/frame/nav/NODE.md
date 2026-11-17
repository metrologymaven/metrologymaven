---
id: OUTR.DOCS.FRAME.NAV
title: Edition build, page frame and navigation
level: 4
parent: OUTR.DOCS.FRAME
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.DOCS.FRAME-002, REQ-OUTR.DOCS.FRAME-006]
covers: []
depends_on: []
uses_artifacts: [OUTR.BRAND.ASSET]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.DOCS.FRAME.NAV: Edition build, page frame and navigation

## Purpose

Builds one release edition of the docs portal: renders the authored Markdown sources (user guide, contributor guide, registered guides) and places the generated reference pages into one frame with a header, a release banner, a navigation tree and a footer, producing only static files whose links all work from a local folder. Every other FRAME child and every DOCS page type uses this build.

## Scope

**In:** the `docs-build` subcommand; the `nav.toml` navigation file; the HTML frame template; Markdown rendering; the release banner; the relative-link rule.
**Out:** edition list, switcher, "latest" alias and out-of-support notices (`OUTR.DOCS.FRAME.ED`); guide registration and outdated markers (`OUTR.DOCS.FRAME.SLOT`); offline bundle archives (`OUTR.DOCS.FRAME.BUNDLE`); reference page content and checks (`OUTR.DOCS.REF`); search (`OUTR.DOCS.SEARCH`); brand assets (`OUTR.BRAND.ASSET`).

## Requirements

- **REQ-OUTR.DOCS.FRAME.NAV-001** When `docs-build` runs for a release, it shall write one HTML page per Markdown source and per reference page under `<out>/<release>/`, each wrapped in the same frame.
- **REQ-OUTR.DOCS.FRAME.NAV-002** Every page frame shall show the release tag it documents in a banner at the top of the page.
- **REQ-OUTR.DOCS.FRAME.NAV-003** The frame shall contain an `<!-- edition-notice -->` placeholder directly below the banner, which the edition step may replace.
- **REQ-OUTR.DOCS.FRAME.NAV-004** The navigation tree shall be generated from `nav.toml`, and the build shall fail if `nav.toml` names a source that does not exist.
- **REQ-OUTR.DOCS.FRAME.NAV-005** Every internal link written by the build shall be relative and shall end in a file name (never a bare directory), so that the edition can be browsed from local files.
- **REQ-OUTR.DOCS.FRAME.NAV-006** The build output shall contain only HTML, CSS, image, font, media, JSON and JavaScript data files, and no file that requires server-side execution.
- **REQ-OUTR.DOCS.FRAME.NAV-007** Two builds of the same sources and release shall produce byte-identical output.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/docs/frame/src` (three book pages, one contributor page, two reference pages, a `nav.toml`), when `fid-outreach docs-build --src src --ref ref --release v0.2.0 --out site` runs, then `site/v0.2.0/` holds six HTML pages, each with a banner reading `v0.2.0` and the notice placeholder.
- **AC-2** Given the AC-1 output, when a test parses every `href` and `src`, then none is absolute or protocol-relative and none ends in `/`.
- **AC-3** Given the AC-1 output opened through `file://` in a headless browser with networking disabled, when the test follows every navigation link, then every target loads.
- **AC-4** (error path) Given fixture `nav-missing` (`nav.toml` names `book/ghost.md`), when the build runs, then it exits 1 naming `book/ghost.md` and writes nothing.
- **AC-5** (determinism) Given AC-1 run twice, then the two output trees are byte-identical.

## Design

- **Command:** `fid-outreach docs-build --src <dir> --ref <dir> --release <tag> --out <dir> [--guides <guides.toml>]`. Project tooling; nearest class W1 (writes into a local output directory).
- **Inputs:** `--src` holds `book/`, `contributor/`, `nav.toml` and `assets/` (brand assets copied in); `--ref` holds reference pages placed by OUTR.DOCS.REF; `--guides` is passed through to the SLOT step.
- **Rendering:** CommonMark with tables and fenced code (the same Markdown library as the crate's `claim_lint`), HTML-escaped text, no raw HTML passthrough except the capability-tag comments; heading anchors from a fixed slug rule.
- **Frame:** one template with header (project placeholder name until D-002), banner, notice placeholder, navigation, content and footer; one CSS file; no JavaScript except the search files added by OUTR.DOCS.SEARCH.
- **Errors:** `NavSourceMissing`, `SourceUnreadable`, `OutputExists` (refuses to write into a non-empty edition directory).
- **Determinism:** D0. Sorted traversal, no timestamps in output, fixed file modes.
- **Performance budget:** a 2,000-page edition in under 20 s on a PLAT reference machine [unverified until measured].
- **Crate:** `fid-outreach`, module `docs_build`.

## Test fixtures

Synthetic sources under `fixtures/outr/docs/frame/` (`src`, `ref`, `nav-missing`). CC-BY-4.0.

## Build prompt

> Build `fid-outreach docs-build --src <dir> --ref <dir> --release <tag> --out <dir> [--guides <file>]` in the Rust outreach tooling crate `fid-outreach`. It renders every Markdown file under `src/book/` and `src/contributor/` (CommonMark plus tables and fenced code, the same library the crate's `claim_lint` uses; escape text; keep `<!-- cap: ... -->` comments) and copies every page under `--ref` into `<out>/<release>/`, wrapping each in one HTML frame: header, a banner showing the release tag, an `<!-- edition-notice -->` placeholder right below it, a navigation tree generated from `src/nav.toml`, the content, a footer. Copy `src/assets/` once. All internal links must be relative and end in a file name (`index.html`, never `dir/`), so the edition works from `file://`. Output only static files. Fail (exit 1, write nothing) if `nav.toml` names a missing source; refuse a non-empty edition directory. Two builds must be byte-identical (sorted traversal, no timestamps).
>
> Acceptance tests with fixtures under `fixtures/outr/docs/frame/`: six pages with banner and placeholder; no absolute or directory links; every navigation link loads via `file://` with networking disabled in a headless browser; missing nav source exits 1; byte-identical reruns.
>
> Constraints: no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded reads (5 MB per source); no network; BTreeMap only; allowlisted dependency licences.

## Open questions

- OUTR.SITE.GEN builds the website with its own pipeline; sharing one renderer between the site and the portal is a possible later refactor, not assumed here.
- The project name in the header stays a placeholder until D-002's name is chosen.

## Children

_Leaf: none._
