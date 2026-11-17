---
id: ECO.REL
title: Packaging, signed installers and release channels
level: 2
parent: ECO
kind: subsystem
status: open
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-ECO-007]
covers: []
depends_on: [ECO.APPS, ECO.CI, ECO.PARITY, PLAT.MATRIX]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.REL: Packaging and releases

## Purpose

Turns a tagged revision into signed, checksummed installers for Windows, Linux and macOS, a self-hostable web build, and published SDK crates, through a documented, reproducible release process with channels. It owns building and signing the artefacts; PLAT owns which targets must be supported and how they are deployed in the field.

## Scope

**In:**
- Release process document and checklist
- Version numbering and release channels (nightly, beta, stable)
- Per-OS installer and package builds for each app plus the `fid` CLI and `fid-verify`
- Self-hostable web (WASM) bundle
- Code signing and notarisation of release artefacts, checksums, SBOM
- Reproducible-build target and its verification
- Release notes assembled from merged PRs, plus the generated parity and status reports
- Update notification that is off by default and never phones home unless enabled

**Out** (and which node owns it instead):
- Support matrix, tiers, silent enterprise and air-gapped install behaviour (owned by `PLAT`)
- Result signing and `fid-verify` behaviour (owned by `TRUST`)
- Download pages and announcements (owned by `OUTR`)
- Report generation (owned by `ECO.PARITY`)
- Plugin package signing policy (owned by `ECO.SDK`)

## Requirements

- **REQ-ECO.REL-001** Each release shall ship installers for every Tier 1 target in the PLAT support matrix, plus a self-hostable web bundle.
- **REQ-ECO.REL-002** Every release artefact shall be signed with the project's release key and listed with a SHA-256 checksum in a signed checksum file.
- **REQ-ECO.REL-003** Each release shall publish a software bill of materials covering every bundled dependency.
- **REQ-ECO.REL-004** The release process shall be documented step by step, and every step except signing approval shall run in CI from a tag.
- **REQ-ECO.REL-005** Signing release artefacts shall be a human-approved step, and no agent shall hold or trigger release signing keys.
- **REQ-ECO.REL-006** When the same tag is built twice on the reference builder, the non-signature bytes of each artefact shall be identical [unverified for all three OS installers].
- **REQ-ECO.REL-007** The project shall maintain nightly, beta and stable channels, and an installed app shall not check for updates unless the user enables it.
- **REQ-ECO.REL-008** The web bundle shall be servable from any static host with no server-side code and no third-party requests.

## Interfaces

- **Offers:** release artefacts, checksum file, SBOM, release notes, published SDK crates, the release runbook.
- **Consumes:** PLAT support matrix; ECO.APPS app list; ECO.CI build workflows; ECO.PARITY reports; D-002 (product name on installers).

## Decomposition hints

PROC (process, versioning, channels), PKG (per-OS installers), WEB (WASM bundle), SIGN (artefact signing, checksums, SBOM), REPRO (reproducibility), NOTES (release notes).

## Open questions

- Signing certificates for Windows and macOS cost money and need an accountable legal entity; who holds them depends on D-003 (governance) [unverified cost].
- D-002 (name) must settle before the first public installer.

## Children

_None yet: status `open`._
