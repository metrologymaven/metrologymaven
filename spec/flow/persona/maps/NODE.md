---
id: FLOW.PERSONA.MAPS
title: Persona journey maps
level: 3
parent: FLOW.PERSONA
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-FLOW.PERSONA-002, REQ-FLOW.PERSONA-006, REQ-FLOW.PERSONA-007]
covers: [JOB.PER.*]
depends_on: []
uses_artifacts: [FLOW.PERSONA.SCHEMA, FLOW.PERSONA.PERS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# FLOW.PERSONA.MAPS: Persona journey maps

## Purpose

One `journey-map@1` file per persona, describing the persona-level path from first contact to their recurring job: which app they open, which commands and screens they use, where they decide, and where they hand off to another persona (for example the quality engineer handing a report to the quality manager for release). Each map names the acceptance jobs that exercise it.

## Scope

**In:**
- Nine maps `acceptance/journeys/persona/per-01.json`..`per-09.json` (`kind: persona`)
- An `exercised_by` list of `JOB.ACC.*` IDs per map
- The auditor map restricted to `fid-verify`

**Out:**
- Acceptance-job maps (owned by `FLOW.INSPECT`, `FLOW.REPORT`, `FLOW.PROBE`, `FLOW.NETWORK`)
- The setup map (owned by `FLOW.SETUP.JOURNEY`)
- Checking the maps (owned by `FLOW.PERSONA.CHECK`)

## Requirements

- **REQ-FLOW.PERSONA.MAPS-001** There shall be exactly one current persona map per persona ID in the persona file, each validating against `journey-map@1` with `kind: persona`.
- **REQ-FLOW.PERSONA.MAPS-002** Each persona map shall carry `exercised_by`, and the maps of every persona other than `JOB.PER.06` and `JOB.PER.08` shall list at least one `JOB.ACC.*` ID.
- **REQ-FLOW.PERSONA.MAPS-003** Each step that hands work to another persona shall name that persona's ID in `handoff`.
- **REQ-FLOW.PERSONA.MAPS-004** The `JOB.PER.06` map shall use only `verify.*` commands of `fid-verify` and screens of the verifier, with no step needing a licence, an account or network access.
- **REQ-FLOW.PERSONA.MAPS-005** Every step whose command is an adoption, release, signing or administrator command shall have `human_only: true` and an actor other than `agent`.

## Acceptance criteria

- **AC-1** Given the nine maps, when `fid accept check-journeys --json` runs, then every persona map validates and the check reports no persona without a map.
- **AC-2** Given the `JOB.PER.06` map, when inspected, then every command starts with `verify.`.
- **AC-3** (error path) Given a copy of `per-02.json` with `exercised_by: []`, then the check reports `MAP-UNEXERCISED JOB.PER.02`.

## Design

- **Registry command(s):** none; content files.
- **Data structures:** `journey-map@1` with an extra top-level `exercised_by` list (part of the schema for `kind: persona`).
- **Algorithm:** written by hand from seed rows and the app compositions, in project words.
- **Errors:** none at runtime; findings come from `FLOW.PERSONA.CHECK`.
- **Determinism:** D0.
- **Performance budget:** none.
- **Crate:** none (content under `acceptance/journeys/persona/`).

## Test fixtures

The maps themselves; the persona file from `FLOW.PERSONA.PERS`.

## Build prompt

> Write nine `journey-map@1` files under `acceptance/journeys/persona/` (`per-01.json`..`per-09.json`, kind persona), one per persona in `acceptance/personas/personas.json`. Each map: ordered steps with actor, command or screen, owner node ID, decision, handoff (name the receiving persona ID on hand-offs), human_only true for adoption, release, signing and admin commands; `exercised_by` listing acceptance jobs (01: J2 J3; 02: J1 J7; 03: J5; 04: J4 J10; 05: J2 J6; 07: J8; 09: J9; 06 and 08: none). The auditor map (06) uses only `verify.*` commands, offline, no account. Verify with `fid accept check-journeys` (AC-1..AC-3 of spec node FLOW.PERSONA.MAPS).

## Open questions

- `JOB.PER.06` and `JOB.PER.08` have no acceptance job; adding a verifier job and a driver-SDK job is a seed change for maintainers.

## Children

_Leaf: none._
