---
id: ECO.REL.SIGN
title: Release signing, checksums and SBOM
level: 3
parent: ECO.REL
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO.REL-002, REQ-ECO.REL-003, REQ-ECO.REL-005]
covers: []
depends_on: [ECO.REL.PKG]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.REL.SIGN: Release signing, checksums and SBOM

## Purpose

Signs every release artefact with the project's release key after a human approves, produces a signed SHA-256 checksum file, applies OS code signing and notarisation where the OS requires it, and generates a software bill of materials for every bundled dependency. It keeps release keys out of reach of agents and of unattended CI.

## Scope

**In:**
- Release key custody model and the human approval step
- Signed checksum file covering every artefact
- OS code signing and notarisation (Windows, macOS) and Linux package signing
- SBOM generation per artefact in a standard format [unverified: format choice, for example SPDX or CycloneDX]
- Public-key publication and a verification guide for users

**Out** (and which node owns it instead):
- Result signing and the Result Package (owned by `TRUST`)
- Plugin package signing policy (owned by `ECO.SDK`)
- Dependency licence check (owned by `ECO.CI.LIC`)

## Requirements

- **REQ-ECO.REL.SIGN-001** Every release artefact shall be listed with its SHA-256 in one checksum file, and that file shall be signed with the project release key.
- **REQ-ECO.REL.SIGN-002** Signing shall run only after a maintainer with the release role approves the specific release in the release workflow, and the approval shall be recorded with the approver's identity and time.
- **REQ-ECO.REL.SIGN-003** Release signing keys shall not be readable by agents, by PR workflows or by any workflow not gated on the approval in REQ-002.
- **REQ-ECO.REL.SIGN-004** Each release shall publish an SBOM per artefact listing every bundled dependency with name, version, licence expression and source.
- **REQ-ECO.REL.SIGN-005** Where an OS requires vendor code signing or notarisation for an installer to run without warnings, the installer shall be signed or notarised before publication to the beta or stable channel [unverified cost and entity requirements].
- **REQ-ECO.REL.SIGN-006** The project shall publish its release public keys and a step-by-step guide that lets a user verify an artefact offline.

## Interfaces

- **Offers:** signed checksum file; SBOMs; signed and notarised installers; public keys and verification guide.
- **Consumes:** artefacts from ECO.REL.PKG and ECO.REL.WEB; approval step in the release workflow (ECO.REL.PROC); dependency metadata.

## Decomposition hints

CUSTODY (keys and approval), SUMS (checksum file), OSSIGN (code signing and notarisation), SBOM, GUIDE.

## Open questions

- Vendor code-signing certificates need an accountable legal entity and cost money; who holds them depends on governance (D-003 accepted founder-led start) and on a future legal entity [unverified cost]. This may need a decision record when the first public installer is due.
- D-002 must settle before the first public installer (product name in signing certificates).

## Children

_None yet: status `open`._
