---
id: PLAT.WEB
title: Browser support for the WASM build
level: 2
parent: PLAT
kind: subsystem
status: split
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
- Whether a WebGL2 fallback is worth supporting: VIZ.WEB already specifies one (REQ-VIZ.WEB-003); `PLAT.WEB.GFX` limits WebGL2-only browsers to Tier 2 at most.
- Cross-domain: VIZ.WEB's last fallback (server-rendered stills) conflicts with REQ-PLAT.WEB-003 unless the renderer is local or self-hosted; carried by `PLAT.WEB.GFX` and `PLAT.WEB.OFFLINE`.
- Cross-domain: REQ-PLAT.WEB-008 (non-device workflows in the browser) and CONSTITUTION §2 (WASM build of the apps) versus REQ-VIZ.WEB-002 (web viewer links no metrology kernel). Read together, the read-only viewer is kernel-free while the WASM app builds (ECO) must carry the kernel; this should be stated by VIZ and ECO.
- Cross-domain: `PLAT.WEB.DIFF` needs a `web` availability flag in CORE.REG's manifest schema.

## Split rationale

The split follows **the ways a browser differs from a desktop**: which browsers (BROWSERS), which graphics (GFX), how much memory (LIMITS), how the network and data boundary is kept (OFFLINE), which commands are missing (DIFF), and how all of that is tested (CI). Each child maps to one requirement cluster of the parent and to one set of browser checks. DIFF is a leaf now because it only reads the registry manifest.

Alternatives considered:
- **Split by browser engine** (Chromium / Gecko / WebKit). Rejected: graphics, limits and offline rules apply to every engine; per-engine detail is data in BROWSERS and LIMITS.
- **Fold CI into each sibling.** Rejected: all browser checks share one harness and one gate integration, unlike the desktop where `PLAT.MATRIX.CI` already provides it.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| PLAT.WEB.BROWSERS | Browser tiers and minimum versions | 001 002 | none |
| PLAT.WEB.GFX | Browser graphics requirements and fallback notice | 006 | none |
| PLAT.WEB.LIMITS | Browser memory and file-size limits | 005 | none |
| PLAT.WEB.OFFLINE | Local-first and offline operation in the browser | 003 008 | none |
| PLAT.WEB.DIFF | Generated desktop-versus-web feature difference list | 004 | none |
| PLAT.WEB.CI | Headless browser CI per engine | 007 | none |
