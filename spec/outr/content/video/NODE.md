---
id: OUTR.CONTENT.VIDEO
title: Video production with built-in capture
level: 3
parent: OUTR.CONTENT
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.CONTENT-001, REQ-OUTR.CONTENT-004, REQ-OUTR.CONTENT-005]
covers: []
depends_on: [VIZ.CAPTURE, OUTR.CONTENT.PUB]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.CONTENT.VIDEO: Video production with built-in capture

## Purpose

The pipeline that turns a recording made with the platform's own `capture.*` commands into a published, self-hosted video with captions and a transcript, shipped together with the journal and view track it came from. It is separate because it handles media files and accessibility work that text content does not need.

## Scope

**In:**
- Recording procedure using `capture.*` commands
- Editing limits (cuts and titles allowed; no compositing of material not produced by the platform except title cards)
- Captions and text transcript per video
- Self-hosted publication with the journal and view track alongside

**Out** (and which node owns it instead):
- The capture commands and view-track format (owned by `VIZ.CAPTURE`)
- Hosting and embedding pages (owned by `OUTR.SITE` and `OUTR.DOCS`)
- Mirroring to third-party video platforms (a maintainer choice; see Open questions)

## Requirements

- **REQ-OUTR.CONTENT.VIDEO-001** Every video shall be produced from a recording made with the platform's `capture.*` commands, and the published package shall include the journal and the view track it was recorded from.
- **REQ-OUTR.CONTENT.VIDEO-002** Every video shall have a caption file in a standard text format (for example WebVTT) and a text transcript, both reviewed by a person.
- **REQ-OUTR.CONTENT.VIDEO-003** Every video shall be served from the project's own origin, so that embedding it makes no third-party request.
- **REQ-OUTR.CONTENT.VIDEO-004** Each video shall name its example project, journal and verified release in the content manifest defined by OUTR.CONTENT.PUB.
- **REQ-OUTR.CONTENT.VIDEO-005** If a video's journal replays `DIVERGENT` or `NOT_REPLAYABLE` on a release, then the video page shall show the outdated marker from OUTR.CONTENT.REPRO until the video is re-recorded or re-verified.

## Interfaces

- **Offers:** video packages (media, captions, transcript, journal, view track).
- **Consumes:** VIZ.CAPTURE commands and formats; OUTR.CONTENT.PUB; OUTR.CONTENT.REPRO outdated markers.

## Decomposition hints

REC (recording procedure), CAP (captions and transcripts), PKG (package and hosting).

## Open questions

- Mirroring to video platforms would add third-party trackers on those platforms; a maintainer choice (carried from OUTR.CONTENT).

## Children

_None yet: status `open`._
