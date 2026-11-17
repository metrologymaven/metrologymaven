---
id: ECO.REL
title: Packaging, signed installers and release channels
level: 2
parent: ECO
kind: subsystem
status: split
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
- **REQ-ECO.REL-009** The project shall offer a long-term-support release channel with a published support window, security-only and correctness-only backports, and kernel bit-identity across LTS patch releases.

## Interfaces

- **Offers:** release artefacts, checksum file, SBOM, release notes, published SDK crates, the release runbook.
- **Consumes:** PLAT support matrix; ECO.APPS app list; ECO.CI build workflows; ECO.PARITY reports; D-002 (product name on installers).

## Decomposition hints

PROC (process, versioning, channels), PKG (per-OS installers), WEB (WASM bundle), SIGN (artefact signing, checksums, SBOM), REPRO (reproducibility), NOTES (release notes).

## Split rationale

The split follows the **artefact path from tag to user**: PROC orchestrates the release and its channels; PKG builds installers and WEB the static bundle; SIGN signs, checksums and lists dependencies; REPRO proves the bytes came from the source; UPD is the only in-product piece, telling an opted-in user that a release exists. Human-only signing (REQ-ECO.REL-005) sits in SIGN together with key custody, so exactly one node holds keys.

Alternatives considered:
- **Separate NOTES child** (hint). Rejected: release notes are one generated step of the release workflow and share its inputs; folded into PROC.
- **Update check inside PROC.** Rejected: it is product code with its own side-effect class (X1) and privacy rules, unlike the rest of the pipeline.

## Open questions

- Signing certificates for Windows and macOS cost money and need an accountable legal entity; who holds them depends on governance (D-003) and may need a decision record before the first public installer [unverified cost]. Tracked in ECO.REL.SIGN.
- D-002 (name) must settle before the first public installer; ECO.REL.PKG refuses stable publication while placeholder names remain.
- PLAT.HW.RUN, PLAT.DEPLOY, PLAT.WEB and several OUTR nodes depend on ECO.REL, so ECO.REL children read their data as files rather than declaring dependencies on them.
- ECO.REL.UPD: whether the update policy setting is W2 or A0 is not in CONSTITUTION §3.1.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| ECO.REL.PROC | Release process, versioning, channels and notes | 004 007 | none |
| ECO.REL.PKG | Per-OS installers and packages | 001 | none |
| ECO.REL.WEB | Self-hostable web bundle | 001 008 | none |
| ECO.REL.SIGN | Release signing, checksums and SBOM | 002 003 005 | none |
| ECO.REL.REPRO | Reproducible builds and their verification | 006 | none |
| ECO.REL.UPD | Opt-in update check | 007 | none |
| ECO.REL.LTS | Long-term-support channel | 009 | none |
