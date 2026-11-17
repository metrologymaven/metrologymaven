---
id: ECO.REL.SIGN
title: Release signing, checksums and SBOM
level: 3
parent: ECO.REL
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO.REL-002, REQ-ECO.REL-003, REQ-ECO.REL-005]
covers: []
depends_on: []
uses_artifacts: [ECO.REL.PKG, ECO.REL.WEB, TRUST.SIGN.DSSE, ECO.CI.LIC]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.REL.SIGN: Release signing, checksums and SBOM

## Purpose

Signs every release artefact with the project's release key after a human approves, produces a signed SHA-256 checksum file, applies OS code signing and notarisation where the OS requires it, and generates a software bill of materials for every bundled dependency. It keeps release keys out of reach of agents and of unattended CI.

## Scope

**In:**
- Release key custody model and the human approval step
- Signed checksum file covering every artefact
- OS code signing and notarisation (Windows, macOS) and Linux package signing
- SBOM generation per artefact in SPDX 2.3 JSON (chosen jointly with `QUAL.SBOM`; see ECO.REL.SIGN.SBOM)
- Public-key publication, key transitions and a verification guide for users

**Out** (and which node owns it instead):
- Result signing and the Result Package (owned by `TRUST`)
- Plugin package signing policy (owned by `ECO.SDK`)
- Dependency licence check (owned by `ECO.CI.LIC`)
- SBOM content policy, completeness check, vulnerability gate and VEX (owned by `QUAL.SBOM`)

## Requirements

- **REQ-ECO.REL.SIGN-001** Every release artefact shall be listed with its SHA-256 in one checksum file, and that file shall be signed with the project release key.
- **REQ-ECO.REL.SIGN-002** Signing shall run only after a maintainer with the release role approves the specific release in the release workflow, and the approval shall be recorded with the approver's identity and time.
- **REQ-ECO.REL.SIGN-003** Release signing keys shall not be readable by agents, by PR workflows or by any workflow not gated on the approval in REQ-002.
- **REQ-ECO.REL.SIGN-004** Each release shall publish an SBOM per artefact listing every bundled dependency with name, version, licence expression and source.
- **REQ-ECO.REL.SIGN-005** Where an OS requires vendor code signing or notarisation for an installer to run without warnings, the installer shall be signed or notarised before publication to the beta or stable channel [unverified cost and entity requirements].
- **REQ-ECO.REL.SIGN-006** The project shall publish its release public keys and a step-by-step guide that lets a user verify an artefact offline.

## Interfaces

- **Offers:** `SHA256SUMS`, its DSSE envelope and SSH signature (SUMS, CUSTODY); signed channel feeds, release manifest and evidence statement (CUSTODY); approval record `approval.json` (CUSTODY); OS-signed installers and `signing/os-readiness.json` (OSSIGN); per-artefact SPDX SBOMs (SBOM); `KEYS.md`, public keys and key-transition statements (GUIDE).
- **Consumes (as files):** artefacts from `ECO.REL.PKG` and `ECO.REL.WEB`; DSSE envelope format (`TRUST.SIGN.DSSE`); licence data (`ECO.CI.LIC`); feed and manifest payloads (`ECO.REL.PROC.CHANNEL`); evidence statement (`QUAL.EVID`).

## Decomposition hints

CUSTODY (keys and approval), SUMS (checksum file), OSSIGN (code signing and notarisation), SBOM, GUIDE.

## Open questions

- Vendor code-signing certificates need an accountable legal entity and cost money; who holds them depends on governance (D-003 accepted founder-led start) and on a future legal entity [unverified cost]. ECO.REL.SIGN.OSSIGN can be built and tested with test certificates; a decision record is needed before the first public beta or stable installer, not before implementation.
- D-002 must settle before the first public installer (product name in signing certificates).
- **SBOM format (joint with QUAL.SBOM):** SPDX 2.3 JSON is chosen here. Reasons: the dependency allowlist (D-001, ECO.RULES.SPDX) and licence check already speak SPDX licence expressions; SPDX is published as ISO/IEC 5962 [unverified: that standard's edition corresponds to SPDX 2.2.1, not 2.3]. QUAL.SBOM should narrow its "SPDX or CycloneDX" wording to SPDX when expanded.
- QUAL.SBOM, QUAL.EVID, QUAL.IQOQ and ENT.OPS depend on ECO.REL.SIGN, so SIGN children read their files as artefacts and declare no dependency on them.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| ECO.REL.SIGN.CUSTODY | Release key custody, approval record and statement signing (leaf) | 001 002 003 | none |
| ECO.REL.SIGN.SUMS | Checksum file and checksum statement (leaf) | 001 | none |
| ECO.REL.SIGN.OSSIGN | OS code signing, notarisation and Linux package signing (leaf) | 005 | none |
| ECO.REL.SIGN.SBOM | Per-artefact SPDX SBOM generation (leaf) | 004 | none |
| ECO.REL.SIGN.GUIDE | Public keys, key transitions and offline verification guide (leaf) | 006 | none |
