---
id: OUTR.SITE.STATUS
title: Status, parity and roadmap pages
level: 3
parent: OUTR.SITE
kind: module
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.SITE-002, REQ-OUTR.SITE-004]
covers: []
depends_on: [OUTR.SITE.GEN]
uses_artifacts: [ECO.PARITY, ECO.COMM.SCOPE, OUTR.LAUNCH.PLAN]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.SITE.STATUS: Status, parity and roadmap pages

## Purpose

The pages that state how far the project has come: the status and parity page, which renders ECO's generated parity and the spec tree's coverage reports without editing them, and the roadmap page, which is generated from the spec tree's phase and status fields. They are kept apart from the editorial pages because they contain no hand-written claims at all.

## Scope

**In:**
- Status and parity page per release, rendering ECO.PARITY output and `generated/COVERAGE.md` / `generated/INDEX.md`
- Roadmap page from spec-tree `phase:` and `status:` fields
- Milestone dates on the roadmap, only from the launch plan

**Out** (and which node owns it instead):
- Generating the parity, status and coverage reports (owned by `ECO.PARITY` and `tools/scope.py`)
- Defining milestones and their dates (owned by `OUTR.LAUNCH`)
- Per-app summaries (owned by `OUTR.SITE.PAGES`)

## Requirements

- **REQ-OUTR.SITE.STATUS-001** The status page shall embed the parity and coverage reports of the selected release so that the report text extracted from the built page equals the generated source byte-for-byte.
- **REQ-OUTR.SITE.STATUS-002** The status page shall show, next to each embedded report, its source revision, release tag and generation date as recorded in the OUTR.SITE.GEN build manifest.
- **REQ-OUTR.SITE.STATUS-003** The roadmap page shall list, for each phase P0 to later, the level-1 and level-2 spec nodes tagged with that phase and their current status, generated from the spec tree at the site's build revision.
- **REQ-OUTR.SITE.STATUS-004** If the roadmap page source contains a date that is not recorded as a milestone date in the OUTR.LAUNCH launch plan, then the site build shall fail and name the date.
- **REQ-OUTR.SITE.STATUS-005** The status page shall keep a browsable entry for every published release, so that earlier parity numbers remain visible after a new release.

## Interfaces

- **Offers:** status, parity and roadmap pages with stable URLs per release (linked from app pages, docs and benchmark pages).
- **Consumes:** OUTR.SITE.GEN imports; ECO.PARITY `parity.md` and `status.md`; spec-tree `generated/` reports and node frontmatter; OUTR.LAUNCH launch-plan milestone dates.

## Decomposition hints

PARITY (embed and byte check), ROAD (roadmap generator), DATES (milestone-date check).

## Open questions

- The machine-readable form of launch-plan milestone dates is not yet defined; OUTR.LAUNCH owns it. OUTR.SITE.STATUS.ROAD states an assumed minimal contract (`milestones.toml`: id, app, name, date) for OUTR.LAUNCH.PLAN to confirm.
- Per CONSTITUTION §2.1 the former `depends_on: [ECO.PARITY]` is now `uses_artifacts`, together with the spec-tree index (ECO.COMM.SCOPE) and the launch plan (OUTR.LAUNCH.PLAN), which were previously consumed without a declared edge.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [OUTR.SITE.STATUS.PARITY](parity/NODE.md) | Per-release status page embedding parity and coverage reports (leaf) | 001 002 005 | none |
| [OUTR.SITE.STATUS.ROAD](road/NODE.md) | Roadmap page from spec-tree phases and the milestone-date check (leaf) | 003 004 | none |
