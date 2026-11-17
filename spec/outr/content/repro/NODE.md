---
id: OUTR.CONTENT.REPRO
title: Content replay check across releases
level: 3
parent: OUTR.CONTENT
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.CONTENT-002, REQ-OUTR.CONTENT-003]
covers: []
depends_on: [OUTR.CONTENT.PUB]
uses_artifacts: [CORE.RPL, ECO.REL.PKG, ECO.DATA.EX]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.CONTENT.REPRO: Content replay check across releases

## Purpose

The check that replays every content journal (tutorials, migration guides, videos) against each release candidate and records the replay outcome, and the outdated marker shown on content whose journal no longer replays. It is the mechanism that keeps teaching content honest across releases.

## Scope

**In:**
- Replay of every journal listed in the content manifests against a release candidate
- The content reproducibility report per release
- The outdated marker on content pages for a release
- Output normalisation rules shared with tutorial expected outputs

**Out** (and which node owns it instead):
- The replay engine and outcome definitions (owned by `CORE.RPL`)
- The content manifests (owned by `OUTR.CONTENT.PUB`)
- Re-recording or rewriting outdated content (owned by `OUTR.CONTENT.TUT`, `OUTR.CONTENT.MIG`, `OUTR.CONTENT.VIDEO`)

## Requirements

- **REQ-OUTR.CONTENT.REPRO-001** When a release candidate is tagged, the check shall replay every journal named in the content manifests on that candidate and record each outcome as `IDENTICAL`, `EQUIVALENT`, `DIVERGENT` or `NOT_REPLAYABLE`.
- **REQ-OUTR.CONTENT.REPRO-002** The check shall publish a reproducibility report per release listing each content item, its journal digest and its outcome.
- **REQ-OUTR.CONTENT.REPRO-003** If an item's outcome is `DIVERGENT` or `NOT_REPLAYABLE`, then that item's page in that release's edition shall show an outdated marker linking the report.
- **REQ-OUTR.CONTENT.REPRO-004** When an outdated item is re-verified with outcome `IDENTICAL` or `EQUIVALENT`, its manifest's verified release shall be updated and the marker removed for that release.
- **REQ-OUTR.CONTENT.REPRO-005** The check shall run without network access, using only the release candidate and content manifests and journals.

## Interfaces

- **Offers:** the reproducibility report; outdated markers per content item and release.
- **Consumes:** CORE.RPL replay; ECO.REL release candidates; OUTR.CONTENT.PUB manifests.

## Decomposition hints

RUN (replay harness), REPORT (report and markers).

## Open questions

- CONSTITUTION §2.1 (D-034): the former code dependency on CORE.RPL is now `uses_artifacts`: the harness runs the release candidate's `fid` binary and reads its Replay Report JSON.
- Whether `EQUIVALENT` is acceptable for tutorials with exact expected outputs is for maintainers; the requirement treats it as current.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [OUTR.CONTENT.REPRO.RUN](run/NODE.md) | Content replay harness and reproducibility report (leaf) | 001 002 005 | none |
| [OUTR.CONTENT.REPRO.MARK](mark/NODE.md) | Outdated markers and re-verification updates (leaf) | 003 004 | none |
