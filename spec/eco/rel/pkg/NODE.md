---
id: ECO.REL.PKG
title: Per-OS installers and packages
level: 3
parent: ECO.REL
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO.REL-001]
covers: []
depends_on: [PLAT.MATRIX, ECO.APPS.LIST, ECO.CI.WF]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.REL.PKG: Per-OS installers and packages

## Purpose

Builds the installable packages for each released app, the `fid` CLI and `fid-verify`, for every Tier 1 combination in the PLAT support matrix (and best-effort for Tier 2). It owns the package formats, their layout and metadata, and the installed-app registration that cross-app hand-off uses to find sibling apps.

## Scope

**In:**
- Package formats per OS (for example MSI or MSIX on Windows, a signed disk image or package on macOS, distribution packages and a portable archive on Linux) [unverified final choice]
- Per-user (non-admin) installation as the default where the OS allows it
- Installed-app registration (file associations for the project container, discoverable app list for `ECO.APPS.HANDOFF`)
- `fid-verify` packaged standalone so it can be installed without any app
- Package metadata: name placeholder until D-002, version, licence, homepage

**Out** (and which node owns it instead):
- Which OS and architecture combinations are Tier 1 (owned by `PLAT.MATRIX`)
- Silent enterprise, air-gapped and kiosk deployment behaviour (owned by `PLAT.DEPLOY`)
- Signing and notarisation (owned by `ECO.REL.SIGN`)
- The web bundle (owned by `ECO.REL.WEB`)

## Requirements

- **REQ-ECO.REL.PKG-001** For every Tier 1 combination in the support matrix, the release workflow shall build an installer for each app in the app list whose status is not `planned`, plus the `fid` CLI and `fid-verify`, and the release shall fail if any Tier 1 build fails.
- **REQ-ECO.REL.PKG-002** Every installer shall install without administrator rights by default where the OS allows per-user installation.
- **REQ-ECO.REL.PKG-003** `fid-verify` shall be available as a standalone package that installs no app and requires no licence check (CONSTITUTION §2, C8).
- **REQ-ECO.REL.PKG-004** When an app is installed, it shall register itself so that other installed apps of the family can discover it and its version without network access.
- **REQ-ECO.REL.PKG-005** If any package metadata or installer UI string contains a placeholder product name while D-002 is unresolved, then the build shall mark the release as internal and refuse to publish it to the stable channel.
- **REQ-ECO.REL.PKG-006** An installer shall make no network request during installation.

## Interfaces

- **Offers:** installers and packages per Tier 1 combination; installed-app registry entries.
- **Consumes:** PLAT.MATRIX tier file; ECO.APPS.LIST; ECO.CI.WF build jobs; ECO.APPS.MANIFEST branding slot.

## Decomposition hints

WIN, MAC, LINUX (one per OS family), REGISTER (installed-app registration), VERIFY (standalone fid-verify package).

## Open questions

- Package format choices per OS are leaf-level, but app-store distribution (for example a store listing) would need an account and is a maintainer decision.

## Children

_None yet: status `open`._
