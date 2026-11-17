---
id: PLAT.WEB.OFFLINE
title: Local-first and offline operation in the browser
level: 3
parent: PLAT.WEB
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT.WEB-003, REQ-PLAT.WEB-008]
covers: []
depends_on: [PLAT.WEB.BROWSERS, VIZ.WEB, ECO.REL]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# PLAT.WEB.OFFLINE: Local-first web operation

## Purpose

Keeps the web build true to CONSTITUTION C7: every byte of user data is processed in the browser, nothing is uploaded, and after the first load the app starts and opens local files with no network. It also covers self-hosting, so a site can serve the web build from an intranet server.

## Scope

**In:**
- The no-upload rule and how it is enforced and tested (content security policy, request auditing in CI)
- Offline start after first load (an installable web app with cached assets)
- Opening and saving local files through browser file APIs only
- Self-hosted deployment of the web build on an intranet without internet access

**Out** (and which node owns it instead):
- Building and packaging the web bundle (`ECO.REL`)
- Hosting the public instance (`OUTR`)
- The viewer and loader (`VIZ.WEB`)

## Requirements

- **REQ-PLAT.WEB.OFFLINE-001** The web build shall send no user data to any server, and its content security policy shall allow network requests only to its own origin for static assets.
- **REQ-PLAT.WEB.OFFLINE-002** When a release is built, CI shall open, view and export a fixture project in each Tier 1 browser engine with request auditing and fail if any request carries user data or goes to a non-origin host.
- **REQ-PLAT.WEB.OFFLINE-003** After one successful load with network, the web build shall start and open local files with the network disabled, on every Tier 1 browser.
- **REQ-PLAT.WEB.OFFLINE-004** The project shall document self-hosting the web build on an intranet server with no internet access, and the self-hosted build shall need no external service.

## Interfaces

- **Offers:** offline and no-upload test definitions; self-hosting guide.
- **Consumes:** ECO.REL web bundle; VIZ.WEB; browser storage and file APIs.

## Decomposition hints

CSP (content security policy and request audit), PWA (offline start), SELFHOST (intranet hosting guide and test).

## Open questions

- VIZ.WEB lists server-rendered stills as a last fallback; under REQ-PLAT.WEB.OFFLINE-001 that is only acceptable from a local or self-hosted renderer. Cross-domain issue with VIZ.

## Children

_None yet: status `open`._
