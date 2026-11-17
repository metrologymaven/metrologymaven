---
id: VIZ.CAPTURE.OSCAP
title: Full-window capture through OS APIs
level: 3
parent: VIZ.CAPTURE
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.CAPTURE-002]
covers: []
depends_on: [VIZ.CAPTURE.VIDEO, VIZ.CAPTURE.REC]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# VIZ.CAPTURE.OSCAP: Full-window capture through OS APIs

## Purpose

Capture the whole app window (panels included, not just the viewport) through each operating system's screen-capture API, feeding the same segment, timestamp and redaction pipeline as viewport video. It is a later phase because it needs per-OS permissions.

## Scope

**In:**
- Per-OS capture back-ends (Windows, macOS, Linux) for the app's own windows only
- Permission prompts and failure reporting
- Redaction of marked panels before encoding
- Feeding frames into the VIDEO segment pipeline with journal-clock timestamps

**Out** (and which node owns it instead):
- Segments and attachment (`VIZ.CAPTURE.VIDEO`)
- Policy (`VIZ.CAPTURE.REC`)
- OS support tiers and permissions matrix (`PLAT`)

## Requirements

- **REQ-VIZ.CAPTURE.OSCAP-001** Full-window frames shall carry timestamps from the journal's monotonic clock.
- **REQ-VIZ.CAPTURE.OSCAP-002** Capture shall be limited to the app's own windows and shall never capture other applications or the whole desktop.
- **REQ-VIZ.CAPTURE.OSCAP-003** If the OS denies capture permission, then the system shall report the denial and continue viewport-only capture.
- **REQ-VIZ.CAPTURE.OSCAP-004** Panels marked for redaction shall be blanked before encoding.

## Interfaces

- **Offers:** OS frame sources for the VIDEO pipeline.
- **Consumes:** `VIZ.CAPTURE.VIDEO`, `VIZ.CAPTURE.REC`.

## Decomposition hints

WIN, MAC, LINUX, PERM.

## Open questions

- Tagged P2 following the concept doc [unverified]; per-OS API names are background only and need checking.

## Children

_None yet: status `open`._
