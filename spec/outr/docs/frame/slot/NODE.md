---
id: OUTR.DOCS.FRAME.SLOT
title: Guide registration, navigation slots and outdated markers
level: 4
parent: OUTR.DOCS.FRAME
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-OUTR.DOCS.FRAME-004]
covers: []
depends_on: [OUTR.DOCS.FRAME.NAV, OUTR.CONTENT.PUB]
uses_artifacts: [OUTR.CONTENT.REPRO.MARK]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.DOCS.FRAME.SLOT: Guide registration, navigation slots and outdated markers

## Purpose

The registration file through which OUTR.CONTENT places tutorials, migration guides and videos into a release edition, the navigation slots they appear in, and the rendering of the outdated marker on their pages from the release's content status file. Only registered guides appear in navigation, so a draft never leaks into an edition.

## Scope

**In:** the `guides.toml` format; the Tutorials, Migration guides and Videos navigation slots; inserting the outdated marker into a guide page; the registration check inside `docs-build`.
**Out:** the guides themselves (`OUTR.CONTENT.TUT`, `OUTR.CONTENT.MIG`, `OUTR.CONTENT.VIDEO`); content manifests (`OUTR.CONTENT.PUB`); the content status file (`OUTR.CONTENT.REPRO.MARK`); the frame and build (`OUTR.DOCS.FRAME.NAV`).

## Requirements

- **REQ-OUTR.DOCS.FRAME.SLOT-001** Each release edition shall read one `guides.toml` listing registered guides with `slot` (`tutorials`, `migration` or `videos`), `title` and `source` path.
- **REQ-OUTR.DOCS.FRAME.SLOT-002** When the edition is built, the navigation shall list exactly the registered guides in their slots, sorted by title, and a guide source not listed in `guides.toml` shall produce no page and no navigation entry.
- **REQ-OUTR.DOCS.FRAME.SLOT-003** If a registered source does not exist or its manifest `kind` does not match its slot, then the build shall fail and name the entry.
- **REQ-OUTR.DOCS.FRAME.SLOT-004** When a content status file is given and marks a registered guide `outdated`, its page shall show the status file's marker text and link to the reproducibility report directly below the edition banner.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/docs/slot/ok` (two tutorials and one migration guide registered, one unregistered draft), when `docs-build ... --guides guides.toml` runs, then navigation shows the three registered guides in their slots and no page exists for the draft.
- **AC-2** Given fixture `slot/kind-mismatch` (a migration guide registered under `tutorials`), when the build runs, then it exits 1 naming the entry.
- **AC-3** Given fixture `slot/ok` and a content status file marking one tutorial `outdated`, when the build runs with `--content-status`, then that page contains the marker text and a relative link to the report, and the other pages contain none.
- **AC-4** (determinism) Given AC-3 run twice, then the outputs are byte-identical.

## Design

- **Interface:** extends `fid-outreach docs-build` with `--guides <guides.toml>` and `--content-status <file>`; nearest class W1 (part of the local build).
- **`guides.toml`:** `version`, `[[guide]] slot, title, source`.
- **Marker:** an HTML block `<div class="outdated" role="note">` with the escaped marker text from the status file and a relative link to `repro-<release>.html`.
- **Errors:** `GuideSourceMissing(entry)`, `GuideKindMismatch(entry)`, `StatusParse`.
- **Determinism:** D0.
- **Crate:** `fid-outreach`, module `docs_slots` (called from `docs_build`).

## Test fixtures

Synthetic content roots, registration files and status files under `fixtures/outr/docs/slot/`. CC-BY-4.0.

## Build prompt

> Extend `fid-outreach docs-build` in the Rust outreach tooling crate `fid-outreach` with a `docs_slots` module and two options. `--guides <guides.toml>` (`version`, `[[guide]] slot = "tutorials" | "migration" | "videos", title, source`): render only registered sources, list them in three navigation slots sorted by title, and fail (exit 1, naming the entry, writing nothing) if a source is missing or its manifest `kind` (read with the crate's `content_check` parser: tutorial, migration, video) does not match the slot. `--content-status <file>` (written by `content-status`: `items[] {file, state, marker}`): for each registered guide with state `outdated`, insert `<div class="outdated" role="note">` with the escaped marker text and a relative link to `repro-<release>.html` right below the banner.
>
> Acceptance tests under `fixtures/outr/docs/slot/` as listed in the spec (registered only, kind mismatch, marker on the outdated page only, byte-identical reruns).
>
> Constraints: no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded parsing; escape all text; no network; BTreeMap only; allowlisted dependency licences.

## Open questions

- Video pages embed the snippet from `video-embed` (OUTR.CONTENT.VIDEO.PKG), which already carries the marker; the frame marker is then shown once, not twice: video pages use the snippet's marker only.

## Children

_Leaf: none._
