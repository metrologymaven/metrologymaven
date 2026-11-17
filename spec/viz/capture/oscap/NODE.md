---
id: VIZ.CAPTURE.OSCAP
title: Full-window capture through OS APIs
level: 3
parent: VIZ.CAPTURE
kind: module
status: split
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
updated: 2026-11-17
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

- Tagged P2 following the concept doc [unverified]; per-OS API names in the backend leaves are [unverified] and need checking against platform documentation.
- Split: one OS-independent source leaf (own-windows-only handle type, fallback, redaction, timestamps) plus one backend leaf per OS, each in its own crate so `unsafe` OS bindings stay isolated. Full-window capture is requested through `capture.start_recording@1` with source `window_video` (`VIZ.CAPTURE.REC.SESSION`).
- On Wayland, the backend composes the app's own UI instead of using the screencast portal, which cannot be limited to one window [unverified].

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| VIZ.CAPTURE.OSCAP.SRC | Window frame source contract, permission fallback and redaction (leaf) | 001 002 003 004 | none |
| VIZ.CAPTURE.OSCAP.WIN | Windows window-capture backend (leaf) | 002 | none |
| VIZ.CAPTURE.OSCAP.MAC | macOS window-capture backend (leaf) | 002 | none |
| VIZ.CAPTURE.OSCAP.LINUX | Linux window-capture backend (leaf) | 002 | none |
