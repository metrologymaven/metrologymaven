---
id: OUTR.CONTENT.VIDEO.PKG
title: Video package, self-hosting and outdated marker check
level: 4
parent: OUTR.CONTENT.VIDEO
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.CONTENT.VIDEO-001, REQ-OUTR.CONTENT.VIDEO-003, REQ-OUTR.CONTENT.VIDEO-004, REQ-OUTR.CONTENT.VIDEO-005]
covers: []
depends_on: [OUTR.CONTENT.PUB, OUTR.CONTENT.VIDEO.REC, OUTR.CONTENT.VIDEO.CAP]
uses_artifacts: [OUTR.CONTENT.REPRO.MARK, VIZ.CAPTURE.TRACK]
side_effect: [R0, W1]
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.CONTENT.VIDEO.PKG: Video package, self-hosting and outdated marker check

## Purpose

Defines the published video package (media, captions, transcript, edit list, journal and view track, plus the `video.md` manifest) and the check that it is complete, and generates the embed snippet that pages use: a plain HTML5 video element with same-origin relative URLs and the outdated marker taken from the release's content status file. It is the last gate before a video is published on the project's own origin.

## Scope

**In:** the package layout; the `video-package` check; the `video-embed` snippet generator; the same-origin rule for every URL a video page loads.
**Out:** recording and edit list (`OUTR.CONTENT.VIDEO.REC`); captions (`OUTR.CONTENT.VIDEO.CAP`); the manifest fields shared by all content (`OUTR.CONTENT.PUB`); the outdated status file (`OUTR.CONTENT.REPRO.MARK`); page frames that place the snippet (`OUTR.DOCS.FRAME.SLOT`, `OUTR.SITE.PAGES`); the off-origin crawl (`OUTR.SITE.QA`).

## Requirements

- **REQ-OUTR.CONTENT.VIDEO.PKG-001** A video package shall contain `video.md` (the content manifest with `kind: video`), the encoded media file, `captions.vtt`, `transcript.txt`, `edit.toml`, the exported journal and the exported view track, and the check shall report every missing file.
- **REQ-OUTR.CONTENT.VIDEO.PKG-002** The check shall run the edit-list and caption checks of the same crate on the package and report their findings.
- **REQ-OUTR.CONTENT.VIDEO.PKG-003** If `video.md` or any package file contains an absolute URL (scheme or protocol-relative) in a place a page would load (media, track or poster source), then the check shall report `off_origin_url`.
- **REQ-OUTR.CONTENT.VIDEO.PKG-004** When `video-embed` runs, it shall write an HTML snippet that uses only relative URLs to the package's media and caption files, includes the captions as a default text track, and includes the marker text from the given content status file when the video's state is `outdated`.
- **REQ-OUTR.CONTENT.VIDEO.PKG-005** The check shall exit 0 with no errors, 1 with any error, and 2 on unreadable input.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/video/pkg-ok` (complete package passing the REC and CAP checks), when `fid-outreach video-package --package pkg-ok --roster roster.txt --author alice --json` runs, then it exits 0.
- **AC-2** Given fixture `pkg-no-track` (view track missing), when the check runs, then it reports `file_missing` naming the view track.
- **AC-3** Given fixture `pkg-cdn` (`video.md` poster pointing to `https://cdn.example.org/p.jpg`), when the check runs, then it reports `off_origin_url`.
- **AC-4** Given `pkg-ok` and a content status file marking it `outdated`, when `fid-outreach video-embed --package pkg-ok --status content-status-v0.4.0.json --out embed.html` runs, then the snippet contains the marker text and every `src` in it is relative.
- **AC-5** Given `pkg-ok` and a status file marking it `current`, when `video-embed` runs, then the snippet has no marker.
- **AC-6** (determinism) Given AC-4 run twice, then the snippets are byte-identical.

## Design

- **Commands:** `fid-outreach video-package --package <dir> --roster <file> --author <id> [--json]` (R0) and `fid-outreach video-embed --package <dir> --status <file> --out <file>` (nearest class W1: writes one snippet file locally). Not registry commands.
- **Layout:** `video.md`, `media.webm` or `media.mp4` [unverified: container choice made in the procedure], `captions.vtt`, `transcript.txt`, `edit.toml`, `caption-review.toml`, `journal/`, `viewtrack/`, `segments/`.
- **Snippet:** `<figure>` with `<video controls preload="metadata">`, `<source src="./media...">`, `<track kind="captions" default src="./captions.vtt">`, a transcript link, and an optional `<p class="outdated">` marker; no scripts.
- **Errors:** findings `file_missing`, `off_origin_url`, plus REC and CAP findings; fatal `InputUnreadable`, `StatusParse`.
- **Determinism:** D0.
- **Crate:** `fid-outreach`, module `video_pkg`.

## Test fixtures

Synthetic packages under `fixtures/outr/video/` (`pkg-ok`, `pkg-no-track`, `pkg-cdn`) built from the REC and CAP fixtures, with tiny placeholder media bytes. CC-BY-4.0.

## Build prompt

> Build two subcommands in the Rust outreach tooling crate `fid-outreach`, reusing its `content_check`, `video_edl` and `video_captions` modules.
>
> `fid-outreach video-package --package <dir> --roster <file> --author <id> [--json]`: require `video.md` (manifest, `kind: video`), one media file (`media.webm` or `media.mp4`), `captions.vtt`, `transcript.txt`, `edit.toml`, `caption-review.toml`, `journal/`, `viewtrack/`, `segments/`; report `file_missing` for each absent item; run the edit-list and caption checks and include their findings; report `off_origin_url` for any absolute or protocol-relative URL in media, track or poster sources in `video.md`. Exit 0/1/2.
>
> `fid-outreach video-embed --package <dir> --status <content-status json> --out <file>`: write a script-free HTML snippet: `<figure>`, `<video controls preload="metadata">` with a relative `<source>`, a default captions `<track>`, a transcript link, and, if the status file marks this package `outdated`, its marker text in `<p class="outdated">`.
>
> Acceptance tests under `fixtures/outr/video/` as listed in the spec (complete package, missing view track, CDN poster, outdated and current snippets, byte-identical reruns).
>
> Constraints: no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded parsing; escape all text in HTML output; no network; BTreeMap only; allowlisted dependency licences.

## Open questions

- Mirroring to third-party video platforms would add their trackers; it remains a maintainer choice (carried from OUTR.CONTENT.VIDEO) and is not covered here.

## Children

_Leaf: none._
