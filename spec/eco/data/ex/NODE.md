---
id: ECO.DATA.EX
title: Example projects with replayable journals
level: 3
parent: ECO.DATA
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO.DATA-006]
covers: []
depends_on: [ECO.DATA.FETCH, CORE.RPL, CORE.PRJ]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.DATA.EX: Example projects with replayable journals

## Purpose

A versioned set of example projects (one or more per app workflow) whose recorded journals replay on each release with outcome `IDENTICAL` or `EQUIVALENT`. They are the shared fixtures for the per-app end-to-end tests, the docs and tutorials, and new contributors who want to reproduce a result.

## Scope

**In:**
- Example-project index: ID, app workflow, datasets used (by catalogue ID), expected replay outcome, owning domain
- Storage of project files and journals, referencing datasets by catalogue ID instead of embedding large files
- The release check that replays every example and records the outcome
- Rules for updating an example when a release changes an outcome on purpose

**Out** (and which node owns it instead):
- Replay engine and outcome classes (owned by `CORE.RPL`)
- Workflow content and expected results for each app (owned by the per-app nodes `ECO.APPS.REPORT`, `ECO.APPS.INSPECT`, `ECO.APPS.PROBE`, `ECO.APPS.NETWORK` and the domains)
- Tutorials built on the examples (owned by `OUTR`)

## Requirements

- **REQ-ECO.DATA.EX-001** Each example project shall list the datasets it needs by catalogue ID and digest, and shall contain no copy of a dataset marked `fetch-only`.
- **REQ-ECO.DATA.EX-002** When a release is built, every example project's journal shall be replayed in exact mode, and the release shall fail if any outcome is `DIVERGENT` or `NOT_REPLAYABLE`.
- **REQ-ECO.DATA.EX-003** If a replay outcome changes between releases, then the change shall be accepted only through a PR that updates the example's expected outcome and states the cause.
- **REQ-ECO.DATA.EX-004** There shall be at least one example project for each app in the app list whose phase has started.

## Interfaces

- **Offers:** example-project index; examples bundle per release; replay outcome table.
- **Consumes:** CORE.RPL replay; CORE.PRJ container; ECO.DATA.FETCH; ECO.APPS.LIST.

## Decomposition hints

INDEX (index and storage rules), REPLAY (release replay check), UPDATE (expected-outcome change process).

## Open questions

- Replay of examples that include device transcripts (Probe, Network) depends on HAL simulator replay; until then those examples run in dry-run mode.

## Children

_None yet: status `open`._
