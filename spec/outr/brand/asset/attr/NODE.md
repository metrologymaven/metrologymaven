---
id: OUTR.BRAND.ASSET.ATTR
title: Outreach attribution and font-origin check
level: 4
parent: OUTR.BRAND.ASSET
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-OUTR.BRAND.ASSET-001, REQ-OUTR.BRAND.ASSET-002, REQ-OUTR.BRAND.ASSET-003, REQ-OUTR.BRAND.ASSET-004]
covers: []
depends_on: []
uses_artifacts: [ECO.RULES.SPDX]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.BRAND.ASSET.ATTR: Outreach attribution and font-origin check

## Purpose

One check, run by every outreach build, that proves each image, font and media file in an outreach repository is either original project work or a third-party asset listed in `ATTRIBUTION.md` under an allowed licence, and that no page loads a font from another origin. It is the automatic half of REQ-OUTR.BRAND-002.

## Scope

**In:** the `asset-check` command; the `ATTRIBUTION.md` table columns it reads; the `original` marking convention; the licence check against the asset section of the SPDX allowlist; the font-origin scan of CSS and HTML.
**Out:** the allowlist itself (`ECO.RULES.SPDX`); the product repository's licence and attribution check (`ECO.CI.LIC`); the asset files (`OUTR.BRAND.ASSET.PACK`); running the check in each build (`OUTR.SITE.QA`, `OUTR.DOCS.QA`, `OUTR.CONTENT.PUB`).

## Requirements

- **REQ-OUTR.BRAND.ASSET.ATTR-001** The check shall read the `ATTRIBUTION.md` table with columns path, origin, source, author and licence, where origin is `original` for project work (licence CC-BY-4.0, author "project contributors") or `third-party`.
- **REQ-OUTR.BRAND.ASSET.ATTR-002** If a file with an image, font, audio or video extension in the scanned tree has no row whose path or glob matches it, then the check shall fail with `ATTR-MISSING` and name the file.
- **REQ-OUTR.BRAND.ASSET.ATTR-003** If a `third-party` row's licence is not in the allowlist's `asset_licences`, then the check shall fail with `ATTR-LICENCE` and name the row's path and licence.
- **REQ-OUTR.BRAND.ASSET.ATTR-004** If a `third-party` row lacks a source or author, then the check shall fail with `ATTR-INCOMPLETE`.
- **REQ-OUTR.BRAND.ASSET.ATTR-005** If a CSS `@font-face` source, an HTML `<link>` to a stylesheet or font, or a CSS `@import` names an absolute URL whose origin is not the configured site origin, then the check shall fail with `FONT-REMOTE` and name the file and line.
- **REQ-OUTR.BRAND.ASSET.ATTR-006** If a row names a path that matches no file, then the check shall fail with `ATTR-ORPHAN`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/brand/attr/ok/` (an original SVG, a third-party CC0-1.0 photo and an OFL-1.1 font, all with complete rows) and allowlist `allow.toml`, when `fid-outreach asset-check --root ok/ --allow allow.toml --origin https://example.org --json` runs, then it exits 0.
- **AC-2** (error path) Given `missing/` (a PNG with no row), when the check runs, then it exits 1 with `ATTR-MISSING img/x.png`.
- **AC-3** (error path) Given `nc/` (a row licensed `CC-BY-NC-4.0`), when the check runs, then it exits 1 with `ATTR-LICENCE`.
- **AC-4** (error path) Given `remote-font/` (a CSS file with `@font-face { src: url(https://fonts.example.net/a.woff2) }` on line 3), when the check runs, then it exits 1 with `FONT-REMOTE` at line 3.
- **AC-5** (error path) Given `orphan/` (a row for a deleted file), when the check runs, then it exits 1 with `ATTR-ORPHAN`.
- **AC-6** (error path) Given an unparsable allowlist, when the check runs, then it exits 2.
- **AC-7** (determinism) Given the same tree, when the check runs twice, then the reports are byte-identical.

## Design

- **Command:** `fid-outreach asset-check --root <dir> --allow <file> --origin <url> [--exclude <glob>]... [--json]`. Project tooling; nearest class R0.
- **Extensions scanned:** `svg png jpg jpeg gif webp avif ico woff woff2 ttf otf mp3 ogg wav mp4 webm mov` (case-insensitive); `node_modules`, `.git` and `--exclude` globs skipped.
- **Table parse:** the first Markdown table in `ATTRIBUTION.md` with the five headers in order; glob rows allowed (as in ECO.CI.LIC) so a folder of originals can be one row.
- **Font scan:** CSS tokenised without executing anything; HTML `<link rel="stylesheet|preload">` `href` values; relative URLs and URLs on `--origin` pass.
- **Errors:** `ATTR-MISSING`, `ATTR-ORPHAN`, `ATTR-LICENCE`, `ATTR-INCOMPLETE`, `FONT-REMOTE`, `ATTR-PARSE`; exit 0, 1 findings, 2 unreadable input.
- **Determinism:** D0; sorted walk, BTreeMap only.
- **Crate:** `fid-outreach`, module `asset_check`.

## Test fixtures

Small generated files (a 1-pixel PNG, a minimal SVG, a font file created for the test from an OFL source or a stub with the right header) under `fixtures/outr/brand/attr/` (`ok`, `missing`, `nc`, `remote-font`, `orphan`), with their own `ATTRIBUTION.md`, CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-outreach`, add module `asset_check` and subcommand `fid-outreach asset-check --root <dir> --allow <file> --origin <url> [--exclude <glob>]... [--json]`.
>
> Read `ATTRIBUTION.md` at `--root`: the first Markdown table with headers path, origin, source, author, licence (path may be a glob). `origin` is `original` (project work, licence CC-BY-4.0) or `third-party`. Read the allowlist TOML (`asset_licences` list) from `--allow`.
>
> Findings: `ATTR-MISSING <file>` for any image, font, audio or video file (svg png jpg jpeg gif webp avif ico woff woff2 ttf otf mp3 ogg wav mp4 webm mov) with no matching row; `ATTR-ORPHAN` for a row matching no file; `ATTR-LICENCE` for a third-party row whose licence is not allowlisted; `ATTR-INCOMPLETE` for a third-party row missing source or author; `FONT-REMOTE <file>:<line>` for any CSS `@font-face` src, CSS `@import` or HTML `<link>` stylesheet or font preload with an absolute URL not on `--origin`. Skip `.git`, `node_modules` and `--exclude` globs.
>
> Output sorted findings; RFC 8785 JSON with `--json`; exit 0 clean, 1 findings, 2 unreadable or unparsable input.
>
> Tests with fixtures under `fixtures/outr/brand/attr/`: ok, missing row, non-commercial licence, remote font at line 3, orphan row, broken allowlist (exit 2), byte-identical output.
>
> Constraints: no panics, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded reads; no network; BTreeMap rather than HashMap; allowlisted dependency licences only.

## Open questions

- ECO.CI.LIC already checks that files under configured asset paths in the product repositories have an `ATTRIBUTION.md` row, but not their licence. This leaf checks outreach repositories and the licence column; ECO may later absorb both into one tool. Cross-domain note for ECO.CI.LIC.
- The `origin` column is an addition to the table ECO.CI.LIC reads (it reads only the first column), so the two tools can share one file format.

## Children

_Leaf: none._
