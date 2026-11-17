---
id: OUTR.DOCS.FRAME
title: Portal frame, release editions and offline bundles
level: 3
parent: OUTR.DOCS
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.DOCS-004, REQ-OUTR.DOCS-007]
covers: []
depends_on: [ECO.REL]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.DOCS.FRAME: Portal frame, release editions and offline bundles

## Purpose

The portal's skeleton: the page frame and navigation, one browsable edition per supported release with a version switcher and a "latest" alias, the navigation slots where OUTR.CONTENT guides are placed, and the per-release offline bundle. Every other DOCS child produces pages that this frame holds.

## Scope

**In:**
- Page frame, navigation tree and the release banner on every page
- One edition per release still in support under the ECO release policy; version switcher; "latest" alias
- Navigation slots for tutorials and migration guides authored by OUTR.CONTENT
- Static build of each edition and a downloadable offline bundle per release

**Out** (and which node owns it instead):
- Reference pages (owned by `OUTR.DOCS.REF`)
- User guide and contributor guide text (owned by `OUTR.DOCS.BOOK` and `OUTR.DOCS.CONTRIB`)
- Tutorial and migration guide content (owned by `OUTR.CONTENT`)
- Search (owned by `OUTR.DOCS.SEARCH`)
- Release support policy (owned by `ECO.REL`)

## Requirements

- **REQ-OUTR.DOCS.FRAME-001** The portal shall keep one browsable edition for every release that is in support under the ECO.REL release policy, and shall remove an edition from navigation, but keep it in the archive, when support ends.
- **REQ-OUTR.DOCS.FRAME-002** Every docs page shall show, in its frame, the release it documents, and pages of a release that is out of support shall show a notice linking the latest edition.
- **REQ-OUTR.DOCS.FRAME-003** The "latest" alias shall resolve to the newest stable release edition and shall never point to a pre-release edition.
- **REQ-OUTR.DOCS.FRAME-004** The navigation shall offer slots for tutorials and migration guides that OUTR.CONTENT registers per release, and an unregistered guide shall not appear in navigation.
- **REQ-OUTR.DOCS.FRAME-005** For each release, the docs build shall produce one offline bundle archive with a published SHA-256 checksum that, when unpacked, can be browsed from local files with no network access.
- **REQ-OUTR.DOCS.FRAME-006** The docs portal shall build to static files with no server-side code.

## Interfaces

- **Offers:** the page frame; the edition and version-switcher scheme; a guide-registration file format for OUTR.CONTENT; offline bundles per release.
- **Consumes:** ECO.REL release list, channels and support policy; pages from REF, BOOK, CONTRIB and OUTR.CONTENT; OUTR.BRAND assets.

## Decomposition hints

NAV (frame and navigation), ED (editions, switcher and latest alias), SLOT (guide registration), BUNDLE (offline bundle).

## Open questions

- How long a release stays "in support" is ECO's call (carried from OUTR.DOCS).

## Children

_None yet: status `open`._
