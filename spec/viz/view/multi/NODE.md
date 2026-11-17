---
id: VIZ.VIEW.MULTI
title: Multi-viewport layouts and camera sync
level: 3
parent: VIZ.VIEW
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.VIEW-001, REQ-VIZ.VIEW-002]
covers: [PW.VIZ.02]
depends_on: [VIZ.VIEW.STATE, VIZ.VIEW.CAMERA]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# VIZ.VIEW.MULTI: Multi-viewport layouts and camera sync

## Purpose

Several viewports in one view: split layouts, cameras synced or independent per viewport, and before/after split views (for example a proposal versus the working document, or two alignments). The layout and its per-viewport ViewStates are part of what a NamedView saves and restores.

## Scope

**In:**
- Viewport layout model (single, two-way, four-way, custom splits)
- Camera sync groups (full, rotation only, none)
- Before/after split bound to two document states or branches
- Inclusion of the layout in ViewState for NamedViews

**Out** (and which node owns it instead):
- Window docking and panels (`VIZ.SHELL`)
- Proposal diff content (`VIZ.INBOX`)
- Camera commands (`VIZ.VIEW.CAMERA`)

## Requirements

- **REQ-VIZ.VIEW.MULTI-001** The viewport layout and every viewport's ViewState shall be captured in the saved view state and restored byte-identically.
- **REQ-VIZ.VIEW.MULTI-002** While cameras are synced, a camera change in one viewport shall apply to every viewport in the sync group in the same frame.
- **REQ-VIZ.VIEW.MULTI-003** A before/after split shall label each side with the document state or branch head it shows.
- **REQ-VIZ.VIEW.MULTI-004** The system shall support at least 4 viewports per layout [unverified].

## Interfaces

- **Offers:** `ViewportLayout` type; split-view API used by `VIZ.INBOX` visual diffs.
- **Consumes:** `VIZ.VIEW.STATE`; `VIZ.VIEW.CAMERA`.

## Decomposition hints

LAYOUT, SYNC, SPLIT (before/after).

## Open questions

- Tagged P1b: P1a ships a single viewport; the Inbox visual diff in P1a uses a toggle rather than a split [unverified].

## Children

_None yet: status `open`._
