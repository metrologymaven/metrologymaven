---
id: PLAT.WEB
title: Browser support for the WASM build
level: 2
parent: PLAT
kind: subsystem
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT-008]
covers: []
depends_on: [PLAT.MATRIX, VIZ.WEB, ECO.CI, ECO.REL]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# PLAT.WEB: Browser support

## Purpose

Defines where and how the WASM build of the viewer and the non-device workflows runs in browsers: which browsers and versions are supported, which graphics paths (WebGPU, with any fallback) are required, memory and file-size limits, how local-first and offline behaviour is kept in a browser, and the published list of feature differences from desktop. It is the platform contract for the web target; VIZ and ECO build it.

## Scope

**In:**
- Supported browsers and versions, tiered like the desktop matrix
- Graphics requirements in the browser (WebGPU, and whether a WebGL fallback is supported)
- Browser memory and file-size limits and how the app behaves at them
- Local-first operation in the browser: no server-side processing of user data, offline use after first load
- The published desktop-versus-web feature difference list, generated from the registry
- Browser CI coverage (headless test runs per supported browser engine)

**Out** (and which node owns it instead):
- Building, hosting and self-host packaging of the WASM build (`ECO`)
- The WASM renderer and UI themselves (`VIZ`)
- Device workflows in the browser: not supported; devices need desktop (`HAL`)
- Desktop OS tiers (`PLAT.MATRIX`)

## Requirements

- **REQ-PLAT.WEB-001** The project shall publish the supported browsers, minimum versions and tiers for the WASM build in the support matrix file.
- **REQ-PLAT.WEB-002** The viewer and every non-device workflow shall run in the current stable release of each Tier 1 browser [unverified: Tier 1 browser set to be chosen].
- **REQ-PLAT.WEB-003** The web build shall process all user data in the browser and shall send no user data to any server, per CONSTITUTION C7.
- **REQ-PLAT.WEB-004** The project shall generate the desktop-versus-web feature difference list from the registry manifest, and CI shall fail if a command lacks a web availability flag.
- **REQ-PLAT.WEB-005** If a dataset exceeds the browser's available memory, then the app shall refuse the load with a message stating the limit and the desktop alternative instead of crashing the tab.
- **REQ-PLAT.WEB-006** If WebGPU is unavailable, then the app shall show a notice naming the missing capability and the supported fallback, if any.
- **REQ-PLAT.WEB-007** When a release is built, CI shall run the web test suite headlessly on each Tier 1 browser engine.
- **REQ-PLAT.WEB-008** After the first successful load, the web build shall start and open local files with no network connection.

## Interfaces

- **Offers:** browser rows of the support matrix; the generated feature difference list; web CI job definitions.
- **Consumes:** VIZ WASM viewer and UI; ECO web packaging and hosting; CORE registry manifest (web availability flags); `PLAT.MATRIX` schema.

## Decomposition hints

BROWSERS (browser tiers), GFX (WebGPU and fallback policy), LIMITS (memory and file-size behaviour), OFFLINE (local-first and offline start), DIFF (generated feature difference list), CI (headless browser jobs).

## Open questions

- Phase set to P1b because the desktop viewer lands in P1a; moving it is a steering-group decision.
- Whether a WebGL2 fallback is worth supporting at all is open.

## Children

_None yet: status `open`._
