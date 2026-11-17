---
id: OUTR.CONTENT.REPRO.MARK
title: Outdated markers and re-verification updates
level: 4
parent: OUTR.CONTENT.REPRO
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-OUTR.CONTENT.REPRO-003, REQ-OUTR.CONTENT.REPRO-004]
covers: []
depends_on: [OUTR.CONTENT.PUB]
uses_artifacts: [OUTR.CONTENT.REPRO.RUN]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.CONTENT.REPRO.MARK: Outdated markers and re-verification updates

## Purpose

Turns a release's reproducibility report into a content status file that the docs frame reads to show an outdated marker on each content item that no longer replays, and, for items that replay again, updates `verified_release` in their manifests so the marker disappears. It keeps the marker logic in one place so tutorials, guides and videos all behave the same.

## Scope

**In:** the `content-status` subcommand; the `content-status-<release>.json` file format; the marker text and link target; the manifest update for re-verified items.
**Out:** running replays (`OUTR.CONTENT.REPRO.RUN`); rendering the marker in pages (`OUTR.DOCS.FRAME.SLOT` reads the status file); rewriting outdated content (`OUTR.CONTENT.TUT`, `OUTR.CONTENT.MIG`, `OUTR.CONTENT.VIDEO`).

## Requirements

- **REQ-OUTR.CONTENT.REPRO.MARK-001** When `content-status` runs on a reproducibility report, it shall write a status file listing every content item of the report with state `outdated` if its outcome is `DIVERGENT` or `NOT_REPLAYABLE` and `current` otherwise, plus the release tag and the report's relative URL.
- **REQ-OUTR.CONTENT.REPRO.MARK-002** The status file shall carry, for each outdated item, a fixed marker text naming the release and the outcome, so that every page renders the same words.
- **REQ-OUTR.CONTENT.REPRO.MARK-003** When run with `--update-manifests`, the tool shall set `verified_release` to the report's release in the manifest of every item whose outcome is `IDENTICAL` or `EQUIVALENT`, change no other byte of the file, and leave outdated items untouched.
- **REQ-OUTR.CONTENT.REPRO.MARK-004** If the report names a content file that is not under the given content root, then the tool shall exit 1 and name the file without writing anything.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/repro/mark/report-mixed.json` (one `IDENTICAL`, one `DIVERGENT`, one `NOT_REPLAYABLE`), when `fid-outreach content-status --report report-mixed.json --root content --out out` runs, then `out/content-status-v0.3.0.json` lists one `current` and two `outdated` items with marker texts naming `v0.3.0` and the outcome.
- **AC-2** Given the same inputs with `--update-manifests`, when the tool runs, then only the `IDENTICAL` item's manifest changes, and only in its `verified_release` line.
- **AC-3** (error path) Given fixture `report-unknown-file.json`, when the tool runs, then it exits 1 naming the file and writes nothing.
- **AC-4** (determinism) Given AC-1 run twice, then the status files are byte-identical.

## Design

- **Command:** `fid-outreach content-status --report <file> --root <dir> --out <dir> [--update-manifests] [--json]`. Project tooling; nearest class W1 (writes the status file and, with the flag, manifest lines in the local content repository).
- **Status file:** RFC 8785 JSON `{release, report_url, items:[{file, kind, state, outcome, marker?}]}` sorted by file. Marker text: "This content was last verified on <verified_release>. Its journal replays as <outcome> on <release>; see the reproducibility report." (project words, fixed).
- **Manifest update:** line-oriented replacement of the `verified_release:` line inside front matter only; files are rewritten atomically (write to temp, rename).
- **Errors:** `ReportParse`, `RootUnreadable`, finding `file_outside_root`.
- **Determinism:** D0.
- **Crate:** `fid-outreach`, module `content_status`.

## Test fixtures

Synthetic report and content fixtures under `fixtures/outr/repro/mark/`. CC-BY-4.0.

## Build prompt

> Build the `content-status` subcommand of the Rust outreach tooling crate `fid-outreach`: `fid-outreach content-status --report <repro-<release>.json> --root <content dir> --out <dir> [--update-manifests] [--json]`. Input: the reproducibility report written by `content-replay` (`release`, `items[] {file, kind, journal, journal_sha256, outcome, reason}`). Output: `content-status-<release>.json` (RFC 8785, sorted by file) with `release`, `report_url` (`repro-<release>.html` relative), and for each item `state` = `outdated` if outcome is `DIVERGENT` or `NOT_REPLAYABLE`, else `current`, plus a fixed marker text for outdated items naming the verified release, the outcome and the release. With `--update-manifests`, rewrite only the `verified_release:` front-matter line of items whose outcome is `IDENTICAL` or `EQUIVALENT`, atomically. If any report file is outside `--root`, exit 1 naming it and write nothing. Exit 2 on unreadable input.
>
> Acceptance tests under `fixtures/outr/repro/mark/`: mixed report gives one current and two outdated items; manifest update touches only the one line of the current item; an unknown file exits 1 with no writes; two runs are byte-identical.
>
> Constraints: no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded parsing; no network; BTreeMap only; allowlisted dependency licences.

## Open questions

- The marker wording should be reviewed with OUTR.BRAND for tone; it states facts only.

## Children

_Leaf: none._
