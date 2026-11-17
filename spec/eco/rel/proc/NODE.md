---
id: ECO.REL.PROC
title: Release process, versioning, channels and notes
level: 3
parent: ECO.REL
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO.REL-004, REQ-ECO.REL-007]
covers: []
depends_on: [ECO.CI.WF]
uses_artifacts: [ECO.PARITY.GEN, ECO.PARITY.STATUS, ECO.APPS.LIST, ECO.DATA.BENCH, QUAL.EVID, QUAL.THREAT, QUAL.CRA, QUAL.SBOM, AGENT.REDTEAM, PLAT.MATRIX.CI, FLOW.RUNNER, I18N.XLATE, KRN.MAN.XART]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.REL.PROC: Release process, versioning, channels and notes

## Purpose

The release pipeline as a whole: the written runbook, the version scheme for apps, CLI, verifier and SDK crates, the nightly, beta, stable and LTS channels and their channel feeds, the release gates that must all pass before a person is asked to approve signing, the CI release workflow that runs every step from a tag up to the human signing approval, and release notes assembled from merged PRs plus the generated parity, status and numbers reports.

## Scope

**In:**
- Runbook and checklist (step by step, with who does each step)
- Version scheme: one product version per release train; SDK crates versioned separately under ECO.SDK's semver policy
- Channels: nightly, beta, stable, lts; promotion rules between them; a signed channel feed per channel; the per-release release manifest; the distribution-venue list
- Release gates: one aggregated go/no-go over every gate file other domains publish (evidence completeness, threat-model review per minor, EU distribution counsel gate, red-team verdict, vulnerability gate, platform gate, GA gates, translation gate, kernel patch bit-identity)
- Release workflow orchestration from tag to "awaiting signing approval" to publication
- Release notes generation from merged PR titles and labels, with the parity, status, release-numbers and gate reports attached

**Out** (and which node owns it instead):
- Building installers (owned by `ECO.REL.PKG`) and the web bundle (owned by `ECO.REL.WEB`)
- Signing, checksums and SBOM (owned by `ECO.REL.SIGN`)
- Reproducibility verification (owned by `ECO.REL.REPRO`)
- LTS support window, backport policy and LTS gate (owned by `ECO.REL.LTS`)
- The in-app update check (owned by `ECO.REL.UPD`)
- Producing each gate verdict (owned by `QUAL.EVID`, `QUAL.THREAT`, `QUAL.CRA`, `QUAL.SBOM`, `AGENT.REDTEAM`, `PLAT.MATRIX.CI`, `FLOW.RUNNER`, `I18N.XLATE`, `KRN.MAN.XART`)
- Announcements and download pages (owned by `OUTR`)

## Requirements

- **REQ-ECO.REL.PROC-001** The project shall publish a release runbook in which every step names whether CI or a named role performs it, and only the signing approval step shall be performed by a person.
- **REQ-ECO.REL.PROC-002** When a release tag is pushed, the release workflow shall run every runbook step up to signing approval without manual intervention and stop in an "awaiting signing approval" state.
- **REQ-ECO.REL.PROC-003** Each product release shall carry one version number in the form `MAJOR.MINOR.PATCH` with an optional `-beta.N` or `-nightly.YYYYMMDD` suffix, shared by every app, the `fid` CLI and `fid-verify` in that release.
- **REQ-ECO.REL.PROC-004** A version shall reach the stable channel only after it has been on the beta channel for at least 7 days without an unresolved release-blocking issue [unverified: period to be set by maintainers].
- **REQ-ECO.REL.PROC-005** Each channel shall have a signed feed listing its latest version, release date and artefact URLs with checksums.
- **REQ-ECO.REL.PROC-006** The release notes shall be generated from merged PRs since the previous release on the same channel, grouped by label, and shall attach the parity report, status report and release-numbers bundle for that release.

## Interfaces

- **Offers:** `release/RUNBOOK.md` and `release/runbook.toml` (RUNBOOK); version and tag rules (VERSION); `release-manifest.json` per release, `feeds/<channel>.json` channel feeds and `release/venues.toml` (CHANNEL; consumed by `ECO.REL.UPD`, `OUTR.SITE.DL`, `OUTR.SITE.GEN`, `OUTR.PROG.CONF`, `OUTR.DOCS.FRAME`); `release-gates.json` (GATES); the release workflow and `release-state.json` (FLOW); `release-notes.md` (NOTES).
- **Consumes (as files):** parity and status reports (`ECO.PARITY.GEN`, `ECO.PARITY.STATUS`); `release-numbers.json` (`ECO.DATA.BENCH`); app list (`ECO.APPS.LIST`); gate files from `QUAL.EVID`, `QUAL.THREAT`, `QUAL.CRA`, `QUAL.SBOM`, `AGENT.REDTEAM`, `PLAT.MATRIX.CI`, `FLOW.RUNNER`, `I18N.XLATE`, `KRN.MAN.XART`; artefacts and plans from `ECO.REL.PKG`, `ECO.REL.WEB`, `ECO.REL.SIGN`.
- **Code dependency:** reusable workflows of `ECO.CI.WF`.

## Decomposition hints

RUNBOOK, VERSION, CHANNEL (channels, promotion and feeds), GATES (aggregated release gate), FLOW (release workflow), NOTES.

## Open questions

- Release cadence (time-based or feature-based) is a maintainer call; nothing in the children assumes either.
- **Cross-domain asks folded in:** the three QUAL runbook steps (evidence-bundle completeness, threat-model review per minor, counsel-reviewed EU distribution gate) and the `block` verdict of `AGENT.REDTEAM` are gates in ECO.REL.PROC.GATES and steps in ECO.REL.PROC.RUNBOOK. `QUAL.EVID`, `QUAL.THREAT`, `QUAL.CRA` and `AGENT.REDTEAM` should adopt the gate file paths named in the GATES Design when they are expanded.
- QUAL, ENT.OPS, I18N.XLATE, PLAT.HW.RUN and PLAT.DEPLOY depend on ECO.REL, so every gate input is read as a file (`uses_artifacts`), never as a code dependency.
- Nightly builds are not "releases" under REQ-ECO.REL-002: they are signed with a separate CI-held nightly key, never with the release key (see ECO.REL.PROC.CHANNEL). Maintainers should confirm this reading.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| ECO.REL.PROC.RUNBOOK | Release runbook and its consistency check (leaf) | 001 | none |
| ECO.REL.PROC.VERSION | Version scheme and tag check (leaf) | 003 | none |
| ECO.REL.PROC.CHANNEL | Channels, promotion, release manifest and channel feeds (leaf) | 004 005 | none |
| ECO.REL.PROC.GATES | Aggregated release gate (leaf) | 001 002 | none |
| ECO.REL.PROC.FLOW | Release workflow from tag to publication (leaf) | 002 | none |
| ECO.REL.PROC.NOTES | Release notes generator (leaf) | 006 | none |
