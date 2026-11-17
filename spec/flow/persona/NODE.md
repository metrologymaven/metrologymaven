---
id: FLOW.PERSONA
title: Personas, journey-map format and journey coverage
level: 2
parent: FLOW
kind: subsystem
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-FLOW-001]
covers: [JOB.PER.*]
depends_on: [ECO.APPS]
cli: [fid accept list-journeys, fid accept show-journey, fid accept check-journeys]
mcp: [accept_list_journeys, accept_show_journey, accept_check_journeys]
journal: []
updated: 2026-11-16
---

# FLOW.PERSONA: Personas, journey-map format and journey coverage

## Purpose

Defines who the product serves and the one machine-checkable format every journey map uses. It owns the nine persona definitions and each persona's journey map (the persona-level path through the apps, from first contact to their recurring job). It also owns the format that the job-family siblings use for acceptance-job maps, and the check that every `JOB.*` row has a map whose steps name real owning nodes and registry commands. It is separate from the job families because the format and the coverage check must be one thing, written once, and personas cut across several jobs.

## Scope

**In:**
- Persona definitions for `JOB.PER.01`..`JOB.PER.09`: role, goals, environment (shop floor, office, lab), primary app (D-004), entry phase, governance profile they usually work under
- One persona journey map per persona: ordered steps, each with command or screen, decision point, hand-off and owning node
- The journey-map format (a structured file per map, versioned) shared by every FLOW child
- The journey coverage check: every `JOB.*` seed row has exactly one current map; every step's owning node exists in the spec tree; every named command exists in the registry manifest once that command is built
- Read-only commands to list and show maps

**Out** (and which node owns it instead):
- Maps of individual acceptance jobs `JOB.ACC.*` (owned by `FLOW.INSPECT`, `FLOW.REPORT`, `FLOW.PROBE`, `FLOW.NETWORK`)
- The first-run setup journey (owned by `FLOW.SETUP`)
- Golden journals and their replay (owned by `FLOW.RUNNER`)
- User research to validate personas with practising metrologists (owned by `UX`)
- The app composition each persona uses (owned by `ECO.APPS`)

## Requirements

- **REQ-FLOW.PERSONA-001** The project shall define each of the nine personas `JOB.PER.01`..`JOB.PER.09` with role, goals, working environment, primary focused app, entry phase and usual governance profile.
- **REQ-FLOW.PERSONA-002** Each persona shall have one journey map whose steps are ordered and each name: a registry command or screen, the decision the user makes (if any), the hand-off to the next step, and the owning spec node.
- **REQ-FLOW.PERSONA-003** The journey-map format shall be a versioned, schema-validated structured file, and every journey map in FLOW shall validate against it.
- **REQ-FLOW.PERSONA-004** When `fid accept check-journeys` runs, the system shall report every `JOB.*` seed row without a current map, every step whose owning node ID does not exist in the spec tree, and every step whose command ID is absent from the registry manifest for the target release.
- **REQ-FLOW.PERSONA-005** If a journey-map step names a command whose side-effect class is M1, S1, W3 or A0, then the map shall mark that step as human-only, and the check shall fail if any such step is attributed to an agent.
- **REQ-FLOW.PERSONA-006** Each persona map shall name the acceptance jobs (`JOB.ACC.*`) that exercise it, and every persona except `JOB.PER.06` and `JOB.PER.08` shall be exercised by at least one acceptance job.
- **REQ-FLOW.PERSONA-007** The persona of the auditor (`JOB.PER.06`) shall map onto `fid-verify` only, with no step that requires a licensed or networked component (CONSTITUTION C7, C8).

## Interfaces

- **Offers:** the journey-map schema; persona maps; `accept.list_journeys@1` (R0), `accept.show_journey@1` (R0), `accept.check_journeys@1` (R1, used by CI).
- **Consumes:** the registry manifest (`CORE.REG`) and the spec tree index for the check; app composition (`ECO.APPS`).

## Decomposition hints

SCHEMA (journey-map format and validation), PERS (persona definitions), MAPS (persona journey maps, one per persona, grouped by app), CHECK (coverage check and CI wiring).

## Open questions

- `JOB.PER.08` (instrument-maker) and `JOB.PER.06` (auditor) have no acceptance job of their own. Should a driver-SDK job and a verifier job be added to `seeds/journeys.md`? That is a seed change for maintainers.
- Doc 10 §3.9 names an expert-reviewer persona that is not a seed row; left out until the seed is updated.

## Children

_None yet: status `open`._
