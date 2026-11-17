---
id: ECO.REL.PROC
title: Release process, versioning, channels and notes
level: 3
parent: ECO.REL
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO.REL-004, REQ-ECO.REL-007]
covers: []
depends_on: [ECO.CI.WF, ECO.PARITY.GEN, ECO.APPS.LIST]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.REL.PROC: Release process, versioning, channels and notes

## Purpose

The release pipeline as a whole: the written runbook, the version scheme for apps, CLI, verifier and SDK crates, the nightly, beta and stable channels and their channel feeds, the CI release workflow that runs every step from a tag up to the human signing approval, and release notes assembled from merged PRs plus the generated parity, status and numbers reports.

## Scope

**In:**
- Runbook and checklist (step by step, with who does each step)
- Version scheme: one product version per release train; SDK crates versioned separately under ECO.SDK's semver policy
- Channels: nightly, beta, stable; promotion rules between them; a signed channel feed per channel listing the latest version and artefact URLs
- Release workflow orchestration from tag to "awaiting signing approval" to publication
- Release notes generation from merged PR titles and labels, with the parity, status and release-numbers reports attached

**Out** (and which node owns it instead):
- Building installers (owned by `ECO.REL.PKG`) and the web bundle (owned by `ECO.REL.WEB`)
- Signing, checksums and SBOM (owned by `ECO.REL.SIGN`)
- Reproducibility verification (owned by `ECO.REL.REPRO`)
- The in-app update check (owned by `ECO.REL.UPD`)
- Announcements and download pages (owned by `OUTR`)

## Requirements

- **REQ-ECO.REL.PROC-001** The project shall publish a release runbook in which every step names whether CI or a named role performs it, and only the signing approval step shall be performed by a person.
- **REQ-ECO.REL.PROC-002** When a release tag is pushed, the release workflow shall run every runbook step up to signing approval without manual intervention and stop in an "awaiting signing approval" state.
- **REQ-ECO.REL.PROC-003** Each product release shall carry one version number in the form `MAJOR.MINOR.PATCH` with an optional `-beta.N` or `-nightly.YYYYMMDD` suffix, shared by every app, the `fid` CLI and `fid-verify` in that release.
- **REQ-ECO.REL.PROC-004** A version shall reach the stable channel only after it has been on the beta channel for at least 7 days without an unresolved release-blocking issue [unverified: period to be set by maintainers].
- **REQ-ECO.REL.PROC-005** Each channel shall have a signed feed listing its latest version, release date and artefact URLs with checksums.
- **REQ-ECO.REL.PROC-006** The release notes shall be generated from merged PRs since the previous release on the same channel, grouped by label, and shall attach the parity report, status report and release-numbers bundle for that release.

## Interfaces

- **Offers:** runbook; release workflow; channel feeds (consumed by `ECO.REL.UPD`); release notes.
- **Consumes:** ECO.CI.WF workflows; ECO.PARITY.GEN and ECO.PARITY.STATUS reports; ECO.DATA release-numbers bundle; ECO.APPS.LIST; artefacts from ECO.REL.PKG and ECO.REL.WEB; signatures from ECO.REL.SIGN.

## Decomposition hints

RUNBOOK, VERSION, CHANNEL (channels, promotion and feeds), FLOW (release workflow), NOTES.

## Open questions

- Release cadence (time-based or feature-based) is a maintainer call.
- PLAT.HW.RUN and PLAT.DEPLOY depend on ECO.REL, so release orchestration must not declare dependencies on them; their outputs are consumed as files.

## Children

_None yet: status `open`._
