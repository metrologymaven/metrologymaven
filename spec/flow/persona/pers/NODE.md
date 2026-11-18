---
id: FLOW.PERSONA.PERS
title: Persona definitions file
level: 3
parent: FLOW.PERSONA
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-accept]
built_pr: 90
satisfies: [REQ-FLOW.PERSONA-001, REQ-FLOW.PERSONA-007]
covers: [JOB.PER.*]
depends_on: []
uses_artifacts: [ECO.APPS.LIST, TRUST.PROFILE.DEFS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# FLOW.PERSONA.PERS: Persona definitions file

## Purpose

States who the product serves: one structured record for each of the nine personas `JOB.PER.01`..`JOB.PER.09`, with role, goals, working environment, primary focused app, entry phase and usual governance profile. Every journey map and the study protocol refer to personas by these IDs.

## Scope

**In:**
- `acceptance/personas/personas.json` (`persona-set@1`) and its schema
- The nine persona records, written in project words from the seed rows

**Out:**
- Persona journey maps (owned by `FLOW.PERSONA.MAPS`)
- Validating personas with practising metrologists (owned by `UX`)
- The app list (owned by `ECO.APPS.LIST`) and governance profiles (owned by `TRUST.PROFILE.DEFS`)

## Requirements

- **REQ-FLOW.PERSONA.PERS-001** The persona file shall contain exactly one record for each of `JOB.PER.01`..`JOB.PER.09` and no other ID.
- **REQ-FLOW.PERSONA.PERS-002** Each record shall hold `id`, `role` (at most 120 characters), `goals` (1 to 5 items), `environment` (`shop_floor`, `office`, `lab` or `field`), `primary_app` (an app `id` from the app list, or `verifier` or `sdk`), `entry_phase` (a CONSTITUTION §7 phase) and `governance_profile` (`lean`, `regulated` or `assistive_only`).
- **REQ-FLOW.PERSONA.PERS-003** The auditor record `JOB.PER.06` shall have `primary_app` `verifier` and `environment` `office` or `lab`.
- **REQ-FLOW.PERSONA.PERS-004** The file validator shall fail if a `primary_app` other than `verifier` or `sdk` is absent from the app list file, or a `governance_profile` is not a built-in profile name.

## Acceptance criteria

- **AC-1** Given the shipped `personas.json` and the app list, when `cargo xtask personas-check` runs, then it exits 0 and reports 9 personas.
- **AC-2** (error path) Given `fixtures/flow/personas/missing-07.json`, then it exits 1 with `PERS-MISSING JOB.PER.07`.
- **AC-3** (error path) Given `bad-app.json` (`primary_app: "measure"`), then it exits 1 with `PERS-APP`.

## Design

- **Registry command(s):** none; repository check `cargo xtask personas-check`.
- **Data structures:** `PersonaSet { format: "persona-set@1", personas: Vec<Persona> }` sorted by ID.
- **Algorithm:** bounded parse (256 KiB), schema, cross-reference to the app list file and profile names.
- **Errors:** `PERS-SCHEMA`, `PERS-MISSING`, `PERS-EXTRA`, `PERS-APP`, `PERS-PROFILE`.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-accept` module `persona` (xtask wrapper).

## Test fixtures

Hand-written persona files under `fixtures/flow/personas/`.

## Build prompt

> In crate `fid-accept`, add module `persona` with `persona-set@1` (records: id `JOB.PER.NN`, role <= 120 chars, goals 1..5, environment shop_floor|office|lab|field, primary_app from the app list file or `verifier`/`sdk`, entry_phase P0..later, governance_profile lean|regulated|assistive_only) and `cargo xtask personas-check`. Ship `acceptance/personas/personas.json` with nine records written in original words: 01 quality engineer at a job shop (report, P1a, lean), 02 scan inspector (inspect, P1b, lean), 03 shop-floor operator (inspect kiosk, P1b, lean), 04 CMM programmer (probe, P2, lean), 05 quality manager (report, P1a, regulated), 06 auditor or calibration-lab verifier (verifier, P0, regulated), 07 supplier quality (report, P1a, regulated), 08 instrument-maker engineer (sdk, P2, lean), 09 large-volume metrologist (network, P3, lean). Errors `PERS-SCHEMA|PERS-MISSING|PERS-EXTRA|PERS-APP|PERS-PROFILE`. Tests AC-1..AC-3 of spec node FLOW.PERSONA.PERS.

## Open questions

- Entry phases and profiles above are proposals from the seed notes; maintainers confirm in review.
- Doc 10 §3.9 names an expert-reviewer persona that is not a seed row; left out until the seed changes.

## Children

_Leaf: none._
