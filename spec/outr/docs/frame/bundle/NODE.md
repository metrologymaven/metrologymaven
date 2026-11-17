---
id: OUTR.DOCS.FRAME.BUNDLE
title: Offline bundle per release
level: 4
parent: OUTR.DOCS.FRAME
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-OUTR.DOCS.FRAME-005, REQ-OUTR.DOCS.FRAME-006]
covers: []
depends_on: [OUTR.DOCS.FRAME.NAV]
uses_artifacts: []
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.DOCS.FRAME.BUNDLE: Offline bundle per release

## Purpose

Packs one built edition into a single archive that users download and browse from local files with no network, with a published SHA-256 checksum. The archive is reproducible, so anyone can rebuild it from the same edition and get the same checksum.

## Scope

**In:** the `docs-bundle` subcommand; the archive layout; the checksum file; the static-only check of the edition before packing.
**Out:** building the edition (`OUTR.DOCS.FRAME.NAV`); publishing the archive and checksum on the download page (`OUTR.SITE.DL`); signing (`ECO.REL.SIGN`).

## Requirements

- **REQ-OUTR.DOCS.FRAME.BUNDLE-001** When `docs-bundle` runs on an edition, it shall write one ZIP archive `docs-<release>.zip` whose root folder `docs-<release>/` contains the edition with an `index.html` entry page.
- **REQ-OUTR.DOCS.FRAME.BUNDLE-002** The tool shall write `docs-<release>.zip.sha256` holding the archive's SHA-256 in the `sha256sum` text format.
- **REQ-OUTR.DOCS.FRAME.BUNDLE-003** Two runs on the same edition shall produce byte-identical archives (sorted entries, fixed timestamps and permissions).
- **REQ-OUTR.DOCS.FRAME.BUNDLE-004** If the edition contains a file with an extension outside the static allowlist (html, css, js, json, svg, png, jpg, webp, woff2, vtt, txt, webm, mp4) or any absolute internal link, then the tool shall refuse to pack and name the file.

## Acceptance criteria

- **AC-1** Given the edition built from fixture `fixtures/outr/docs/frame/src`, when `fid-outreach docs-bundle --edition site/v0.2.0 --out dist` runs, then `dist/docs-v0.2.0.zip` and its `.sha256` file exist and the checksum matches.
- **AC-2** Given the archive unpacked into a temporary folder and opened via `file://.../index.html` in a headless browser with networking disabled, when the test follows every navigation link and runs one search query, then every page loads and the search returns results.
- **AC-3** Given AC-1 run twice, then the two archives are byte-identical.
- **AC-4** (error path) Given fixture `fixtures/outr/docs/bundle/php` (edition containing `x.php`), when the tool runs, then it exits 1 naming `x.php` and writes nothing.

## Design

- **Command:** `fid-outreach docs-bundle --edition <dir> --out <dir>`. Project tooling; nearest class W1 (writes into a local output directory).
- **Archive:** ZIP with stored or deflate entries at a fixed compression level, entries in byte-wise sorted path order, timestamp 1980-01-01 00:00, permissions 0644, no extra fields.
- **Errors:** `EditionUnreadable`, findings `not_static(file)`, `absolute_link(file, line)`.
- **Determinism:** D0.
- **Performance budget:** a 200 MB edition packed in under 30 s [unverified until measured].
- **Crate:** `fid-outreach`, module `docs_bundle`.

## Test fixtures

The FRAME.NAV fixture edition, and `fixtures/outr/docs/bundle/php`. CC-BY-4.0.

## Build prompt

> Build `fid-outreach docs-bundle --edition <dir> --out <dir>` in the Rust outreach tooling crate `fid-outreach`. Before packing, scan the edition: every file extension must be in {html, css, js, json, svg, png, jpg, webp, woff2, vtt, txt, webm, mp4} and no `href`/`src` in HTML may be absolute or protocol-relative to the docs origin; otherwise exit 1 naming the file and write nothing. Then write `docs-<release>.zip` (release = edition folder name) with root folder `docs-<release>/`, entries in sorted byte order, fixed timestamp 1980-01-01 00:00, permissions 0644, no extra fields, fixed deflate level, and `docs-<release>.zip.sha256` in `sha256sum` format. Two runs must give byte-identical archives.
>
> Acceptance tests: checksum matches; unpacked bundle browses and searches via `file://` with networking disabled; byte-identical reruns; a `.php` file is refused.
>
> Constraints: no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded reads; no network; BTreeMap only; allowlisted dependency licences.

## Open questions

- AC-2's search step needs OUTR.DOCS.SEARCH; until it exists, the test checks navigation only.

## Children

_Leaf: none._
