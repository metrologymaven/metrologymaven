---
id: PLAT.MATRIX
title: Support matrix: OS and architecture tiers with platform CI
level: 2
parent: PLAT
kind: subsystem
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT-001, REQ-PLAT-002]
covers: []
depends_on: [ECO.CI]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# PLAT.MATRIX: Support matrix and platform CI

## Purpose

Owns the published, machine-readable support matrix: every combination of operating system, OS version and CPU architecture, the tier assigned to it, and the CI coverage each tier demands. It is the single source of truth that the other PLAT children, the docs and `fid doctor` read when they need to know whether a platform is supported. It exists separately because tier policy and CI coverage change on their own cadence, independent of GPU, hardware or deployment work.

## Scope

**In:**
- The support matrix data file: OS family, version range, architecture, tier, phase from which the tier applies, and notes
- Tier definitions (Tier 1 tested in CI on every release, Tier 2 builds in CI, Tier 3 community-maintained)
- Tier promotion and demotion policy, including end-of-life tracking of OS versions
- Platform CI coverage requirements: which jobs must run on which Tier 1 and Tier 2 combinations
- The generated human-readable matrix page
- The Tier 1 set for P1a: Windows 10/11 x86-64, current Ubuntu LTS x86-64, current macOS ARM64
- Tracking of Linux ARM64 and Windows ARM64 tier decisions

**Out** (and which node owns it instead):
- GPU backend support per platform (`PLAT.GPU`)
- Hardware profiles and benchmark machines (`PLAT.HW`)
- Browser support for the WASM build (`PLAT.WEB`)
- Building, hosting and running the CI system itself, installers and signing (`ECO`)
- Checking the local machine against the matrix (`PLAT.DOCTOR`)

## Requirements

- **REQ-PLAT.MATRIX-001** The project shall publish a support matrix in a versioned machine-readable file in which every listed OS, OS version and architecture combination has exactly one tier of Tier 1, Tier 2 or Tier 3.
- **REQ-PLAT.MATRIX-002** The project shall generate the human-readable support matrix page from the machine-readable file, and CI shall fail if the page and the file disagree.
- **REQ-PLAT.MATRIX-003** When a release is built, CI shall build and run the full test suite on every Tier 1 combination, and the release shall be blocked if any Tier 1 job fails.
- **REQ-PLAT.MATRIX-004** When a release is built, CI shall build every Tier 2 combination, and a Tier 2 build failure shall be reported in the release notes without blocking the release.
- **REQ-PLAT.MATRIX-005** From phase P1a, Windows 10 and 11 on x86-64, the current Ubuntu LTS on x86-64 and the current macOS release on ARM64 shall be Tier 1 combinations [unverified until D-005 and CI capacity are confirmed].
- **REQ-PLAT.MATRIX-006** When an OS version reaches its vendor's end of support, the matrix shall demote it from Tier 1 within one release cycle and record the demotion with its date [unverified: release-cycle length not yet set].
- **REQ-PLAT.MATRIX-007** The matrix shall record for each combination the phase from which its tier applies, so that a tier cannot be claimed before its phase.

## Interfaces

- **Offers:** the support matrix file (schema: os, version range, arch, tier, since_phase, notes) read by `PLAT.DOCTOR`, `PLAT.GPU`, `PLAT.HW`, `PLAT.XHOST` and the docs site; the list of required platform CI jobs handed to ECO.
- **Consumes:** ECO CI runners and release pipeline.

## Decomposition hints

FILE (matrix schema and data), POLICY (tier definitions, promotion and EOL rules), CI (required job list and release gate), PAGE (generated docs page).

## Open questions

- Linux ARM64 and Windows ARM64 tiers, and what "current macOS" and "current Ubuntu LTS" mean (latest only or latest two): raised as **D-010**, which blocks part of `PLAT.MATRIX.POLICY`.

## Split rationale

The split is by **artefact of the matrix**: one child owns the file format and its only parser (FILE), one owns what the file says and the rules for changing it (POLICY), and two are pure generators over the file (CI for the job list and release gate, PAGE for the docs page). Every other consumer reads the matrix through FILE, so a format change touches one place.

Alternatives considered:
- **Split by tier** (Tier 1 / Tier 2 / Tier 3). Rejected: each tier needs the same schema, job generator and page; ownership would repeat.
- **Merge CI and PAGE into FILE.** Rejected: the release gate has its own fixtures and failure semantics, and keeping it separate makes each piece a small leaf.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| PLAT.MATRIX.FILE | Support matrix schema, data file and validator | 001 007 | none |
| PLAT.MATRIX.POLICY | Tier definitions, Tier 1 set and end-of-life policy | 005 006 | none |
| PLAT.MATRIX.CI | Platform CI job list and release gate | 003 004 | none |
| PLAT.MATRIX.PAGE | Generated support matrix page and consistency check | 002 | none |
