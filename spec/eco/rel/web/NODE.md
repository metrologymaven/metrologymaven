---
id: ECO.REL.WEB
title: Self-hostable web bundle
level: 3
parent: ECO.REL
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO.REL-001, REQ-ECO.REL-008]
covers: []
depends_on: [VIZ.WEB, ECO.CI.WF]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.REL.WEB: Self-hostable web bundle

## Purpose

Packages the WASM build of the viewer and apps as a static bundle that anyone can host on any static web server, with no server-side code and no request to any third party. It owns the bundle's layout, its content-security rules and the check that proves it makes no outside requests.

## Scope

**In:**
- Static bundle assembly from the WASM builds (viewer, apps that support web)
- No third-party requests: fonts, scripts and assets are bundled
- A content-security policy file and headers guidance for hosts
- A self-test page and an automated check for outbound requests

**Out** (and which node owns it instead):
- The WASM viewer build itself (owned by `VIZ.WEB`)
- Browser support matrix (owned by `PLAT.WEB`)
- Signing and checksums of the bundle archive (owned by `ECO.REL.SIGN`)
- Hosting the project's own demo (owned by `OUTR`)

## Requirements

- **REQ-ECO.REL.WEB-001** Each release shall include a web bundle archive that runs when its files are served by a static file server with no server-side code.
- **REQ-ECO.REL.WEB-002** When the bundle is loaded in a test browser, it shall make no network request to any origin other than the one serving it.
- **REQ-ECO.REL.WEB-003** The bundle shall ship a content-security policy that forbids loading scripts from other origins.
- **REQ-ECO.REL.WEB-004** The bundle shall load and process user files locally in the browser and shall not upload them anywhere.

## Interfaces

- **Offers:** web bundle archive per release; hosting guide.
- **Consumes:** VIZ.WEB WASM build; ECO.CI.WF; PLAT.WEB browser list (as data, to pick test browsers).

## Decomposition hints

ASSEMBLE, CSP, EGRESS (outbound-request check), GUIDE.

## Open questions

- PLAT.WEB depends on ECO.REL, so this node reads PLAT.WEB's browser list as data rather than declaring a dependency.

## Children

_None yet: status `open`._
