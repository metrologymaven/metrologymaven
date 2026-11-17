---
id: PLAT.DEPLOY.KIOSK
title: OS-level kiosk and touchscreen station setup
level: 3
parent: PLAT.DEPLOY
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT.DEPLOY-008]
covers: []
depends_on: [PLAT.MATRIX]
uses_artifacts: [VIZ.KIOSK.SESSION, ECO.REL.PKG.WIN, PLAT.DEPLOY.MCONF]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# PLAT.DEPLOY.KIOSK: OS-level kiosk deployment

## Purpose

Documents and tests how to turn a shop-floor PC or touchscreen panel into a dedicated measuring station: the operator app starts automatically at login in single-app mode, the OS shell is locked down, and the touchscreen is configured for gloved use. VIZ owns kiosk mode inside the app; this node owns everything the OS must do around it.

## Scope

**In:**
- Per Tier 1 desktop OS: auto-start at login, single-app or assigned-access mode, shell lockdown, power and screen-lock settings, restart behaviour
- Touchscreen and on-screen keyboard setup notes for gloved touch
- A smoke test that boots a kiosk-configured VM image and checks the app reaches kiosk mode
- The reference kiosk display assumptions that VIZ.KIOSK cites (size and resolution)

**Out** (and which node owns it instead):
- Kiosk mode inside the app, its UI and its commands (`VIZ.KIOSK`)
- Managed policy settings (`PLAT.DEPLOY.MCONF`)
- Installers (`ECO.REL`)

## Requirements

- **REQ-PLAT.DEPLOY.KIOSK-001** The project shall publish, for each Tier 1 desktop OS, a kiosk deployment guide in which the operator app starts automatically at login of a dedicated account and no other application or OS shell is reachable from the station.
- **REQ-PLAT.DEPLOY.KIOSK-002** When the kiosk station restarts, the operator app shall return to kiosk mode without operator interaction.
- **REQ-PLAT.DEPLOY.KIOSK-003** The project shall name a reference kiosk display (diagonal, resolution, touch technology) used by VIZ.KIOSK's size requirements [unverified: model to be chosen].
- **REQ-PLAT.DEPLOY.KIOSK-004** When a release is built, CI shall boot a kiosk-configured Windows image, wait for login, and check through `fid view get-kiosk-status` that the app is in kiosk mode within 120 seconds [unverified: timing to be measured].

## Interfaces

- **Offers:** kiosk guides and their configuration snippets (`docs/deploy/kiosk/`); `platform/kiosk-display.toml`; the kiosk smoke-test job.
- **Consumes (artifacts):** `view.get_kiosk_status@1` JSON output and `fid view enter-kiosk` behaviour (`VIZ.KIOSK.SESSION`); the Windows MSI (`ECO.REL.PKG.WIN`); the managed policy file format (`PLAT.DEPLOY.MCONF`). Code: the Tier 1 set from `PLAT.MATRIX`.

## Decomposition hints

See Children.

## Open questions

- macOS has limited single-app kiosk support for desktop apps [unverified]; `PLAT.DEPLOY.KIOSK.GUIDE` states reduced guarantees for it.

## Split rationale

Split by **artefact**: written guides (checked by a lint), a release-time CI smoke test on Windows, and one data file naming the reference display. Each has a different consumer (site IT, the release gate, VIZ) and can be built independently. A per-OS split was rejected because only Windows has a CI smoke test, and the guides share one structure and lint.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| PLAT.DEPLOY.KIOSK.GUIDE | Per-OS kiosk deployment guides and guide lint | 001 002 | none |
| PLAT.DEPLOY.KIOSK.SMOKE | Windows kiosk image smoke and restart test | 002 004 | none |
| PLAT.DEPLOY.KIOSK.DISPLAY | Reference kiosk display file | 003 | none |
