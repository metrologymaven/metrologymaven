---
id: PLAT.WEB.LIMITS
title: Browser memory and file-size limits
level: 3
parent: PLAT.WEB
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT.WEB-005]
covers: []
depends_on: [PLAT.WEB.BROWSERS, VIZ.WEB]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# PLAT.WEB.LIMITS: Browser memory and file-size limits

## Purpose

Browsers cap WASM memory and tab memory far below a desktop. This node measures and publishes those limits per browser tier and makes the web app refuse an over-limit load cleanly, with the limit and the desktop alternative, instead of crashing the tab.

## Scope

**In:**
- Measured WASM memory ceilings per Tier 1 browser [unverified until measured]
- The pre-load size estimate rule (file size and declared counts to peak memory)
- Refusal behaviour and message
- Published limits table

**Out** (and which node owns it instead):
- Package loader parsing and bounds (`VIZ.WEB`)
- Desktop memory behaviour (`PLAT.HW`, `VIZ`)

## Requirements

- **REQ-PLAT.WEB.LIMITS-001** The project shall publish, per Tier 1 browser, the maximum dataset size in points and triangles that the web app is tested to load.
- **REQ-PLAT.WEB.LIMITS-002** When a user opens a file, the web app shall estimate peak memory from the file's size and declared counts before allocating, and if the estimate exceeds the available limit it shall refuse the load with a message stating the limit, the estimate and that the desktop app can open it.
- **REQ-PLAT.WEB.LIMITS-003** If an allocation fails during a load despite the estimate, then the web app shall abort the load, free its buffers and show the same message, and the tab shall remain usable.
- **REQ-PLAT.WEB.LIMITS-004** When a release is built, CI shall load a dataset just under and one just over the published limit on each Tier 1 browser engine and check acceptance and clean refusal respectively.

## Interfaces

- **Offers:** limits table; estimate rule; refusal message content.
- **Consumes:** VIZ.WEB loader; browser memory APIs; `PLAT.WEB.CI` jobs.

## Decomposition hints

MEASURE (limit measurement per browser), ESTIMATE (pure estimate function; leaf candidate), REFUSE (refusal flow and message).

## Open questions

- Browser memory ceilings vary by device and change between versions [unverified]; limits may need a safety margin.

## Children

_None yet: status `open`._
