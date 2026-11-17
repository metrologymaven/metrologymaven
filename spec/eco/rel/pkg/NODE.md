---
id: ECO.REL.PKG
title: Per-OS installers and packages
level: 3
parent: ECO.REL
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO.REL-001]
covers: []
depends_on: [ECO.CI.WF]
uses_artifacts: [PLAT.MATRIX.FILE, ECO.APPS.LIST, ECO.APPS.MANIFEST, PLAT.DEPLOY.SILENT, PLAT.DEPLOY.NOADMIN, PLAT.DEPLOY.OFFLINE]
cli: []
mcp: []
journal: []
updated: 2026-11-17
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

- **Offers:** `package-plan.json` and `packages/gate.json` (PLAN); MSI and portable ZIP packages (WIN); DMG and tar.gz packages (MAC); tar.gz, deb and rpm packages (LINUX); the installed-app registration contract and entries (REG, consumed as a file format by `ECO.APPS.HANDOFF`).
- **Consumes (as files):** `platform/support-matrix.toml` (`PLAT.MATRIX.FILE`); `apps.toml` (`ECO.APPS.LIST`); `apps-status.json` and the branding slot (`ECO.APPS.MANIFEST`); MSI property contract (`PLAT.DEPLOY.SILENT`); per-user layout rules (`PLAT.DEPLOY.NOADMIN`); offline-bundle layout (`PLAT.DEPLOY.OFFLINE`).
- **Code dependency:** build jobs of `ECO.CI.WF`.

## Decomposition hints

PLAN (package plan, Tier 1 gate, placeholder-name gate), WIN, MAC, LINUX (one per OS family), REG (installed-app registration). The standalone `fid-verify` package is a row of the plan, built by each OS child.

## Open questions

- Package format choices per OS are made in the OS children; app-store distribution (for example a store listing) would need an account and is a maintainer decision.
- PLAT.DEPLOY.* depend on ECO.REL, so their layout and property contracts are read as files, not declared dependencies.
- Linux package formats were "shared with ECO.REL" in PLAT.DEPLOY.SILENT; ECO.REL.PKG.LINUX picks tar.gz, deb and rpm, and PLAT should align.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| ECO.REL.PKG.PLAN | Package plan, Tier 1 build gate and placeholder-name gate (leaf) | 001 003 005 | none |
| ECO.REL.PKG.WIN | Windows packages (leaf) | 001 002 003 006 | none |
| ECO.REL.PKG.MAC | macOS packages (leaf) | 001 002 003 006 | none |
| ECO.REL.PKG.LINUX | Linux packages (leaf) | 001 002 003 006 | none |
| ECO.REL.PKG.REG | Installed-app registration and file association (leaf) | 004 | none |
