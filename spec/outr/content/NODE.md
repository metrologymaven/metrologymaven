---
id: OUTR.CONTENT
title: Tutorials, migration guides and videos
level: 2
parent: OUTR
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR-005]
covers: []
depends_on: [OUTR.DOCS, OUTR.BRAND, VIZ, ECO]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.CONTENT: Tutorials, migration guides and videos

## Purpose

Teaching content: step-by-step tutorials, migration guides that map an incumbent's workflow onto ours, and videos. Every piece is reproducible from a published example project and journal, and every video is recorded with the platform's own capture commands. It is separate from the docs portal because it is authored content with its own reproducibility checks, published into the portal's frame.

## Scope

**In:**
- Tutorials, one or more per focused app
- Migration guides from incumbents' workflows (scan inspection, CMM programming, large-volume networks, reporting), using the seed inventories as the workflow checklist
- Videos recorded with `capture.*` commands, with captions
- The example project and journal behind each tutorial, guide and video
- The reproducibility check that replays each journal against each release

**Out** (and which node owns it instead):
- Docs portal frame, navigation and reference pages (owned by `OUTR.DOCS`)
- Capture commands themselves (owned by `VIZ`)
- Benchmark datasets and example corpora as such (owned by `ECO`; CONTENT may add its own example projects through ECO's process)
- Benchmark results and case studies (owned by `OUTR.BENCH`)
- Claim lint rules (owned by `OUTR.BRAND`)

## Requirements

- **REQ-OUTR.CONTENT-001** Every tutorial, migration guide and video shall name the published example project and journal it is built from, and the release it was last verified against.
- **REQ-OUTR.CONTENT-002** When a release candidate is tagged, the reproducibility check shall replay every content journal and report each as `IDENTICAL`, `EQUIVALENT`, `DIVERGENT` or `NOT_REPLAYABLE`.
- **REQ-OUTR.CONTENT-003** If a content journal replays as `DIVERGENT` or `NOT_REPLAYABLE` against a release, then the content shall be marked outdated on its page for that release until it is re-verified.
- **REQ-OUTR.CONTENT-004** Every video shall be produced from a recording made with the platform's `capture.*` commands, and shall ship with the journal and view track it was recorded from.
- **REQ-OUTR.CONTENT-005** Every video shall have captions and a text transcript.
- **REQ-OUTR.CONTENT-006** Each migration guide shall map at least the incumbent workflows listed in the matching seed inventory's covered rows to commands of ours, and shall mark every unmapped row as a gap with its spec node.
- **REQ-OUTR.CONTENT-007** Migration guides shall describe incumbents' workflows in our own words, use incumbent product names only to identify them, and contain no copied text, screenshots or icons from incumbent software.
- **REQ-OUTR.CONTENT-008** Every piece of content shall pass the OUTR.BRAND claim lint with zero errors before publication.

## Interfaces

- **Offers:** tutorials and migration guides (published in OUTR.DOCS), videos (embedded on OUTR.SITE and OUTR.DOCS, self-hosted), example projects and journals, the content reproducibility report.
- **Consumes:** `capture.*` commands (VIZ), replay (CORE), release candidates and example corpora (ECO), seed inventories, OUTR.BRAND lint.

## Decomposition hints

TUT (tutorials per app), MIG (migration guides per incumbent), VIDEO (recording and captioning pipeline), REPRO (replay check across releases).

## Open questions

- Phase set to P1a because tutorials need a usable app; content for P0 (CLI and journal walkthroughs) could start earlier.
- Videos must be self-hosted to satisfy REQ-OUTR-003 (no third-party trackers); mirroring to video platforms is a maintainer choice.

## Children

_None yet: status `open`._
