---
id: FLOW.PERSONA.SCHEMA
title: Journey-map format, schema and structural validator
level: 3
parent: FLOW.PERSONA
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-FLOW.PERSONA-003, REQ-FLOW.PERSONA-005]
covers: []
depends_on: [CORE.JRN.CANON]
uses_artifacts: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# FLOW.PERSONA.SCHEMA: Journey-map format, schema and structural validator

## Purpose

Defines `journey-map@1`, the single structured format every journey map in FLOW uses (persona maps, acceptance-job maps and the setup map), its published JSON Schema and a library validator for the rules that need no registry: ordering, required fields, owner-ID syntax and the human-only attribution rule as declared in the map. Registry and spec-tree cross-checks are done by `FLOW.PERSONA.CHECK`.

## Scope

**In:**
- `journey-map@1` fields and `schema/journey-map-v1.json`
- Validator: schema, step numbering, actor and human-only rules, size bounds
- Map digest (RFC 8785 + SHA-256)

**Out:**
- Cross-checks against the manifest and spec tree (owned by `FLOW.PERSONA.CHECK`)
- Map content (owned by `FLOW.PERSONA.MAPS`, the job-family nodes and `FLOW.SETUP.JOURNEY`)

## Requirements

- **REQ-FLOW.PERSONA.SCHEMA-001** A journey map shall hold `format` (`journey-map@1`), `map_id`, `subject` (a `JOB.*` seed ID), `kind` (`persona`, `job` or `setup`), `revision` (integer), `app`, `phase`, `personas` (list of `JOB.PER.*` IDs) and a non-empty ordered `steps` list.
- **REQ-FLOW.PERSONA.SCHEMA-002** Each step shall hold `n` (1-based, contiguous), `actor` (`human`, `operator`, `admin`, `agent` or `system`), exactly one of `command` (`domain.verb_object@major`) or `screen` (a screen ID), `owner` (a spec node ID), `handoff` (text or the next step's `n`), optional `decision`, `human_only` (boolean) and optional `phase` overriding the map's phase.
- **REQ-FLOW.PERSONA.SCHEMA-003** If a step is marked `human_only: true` and its `actor` is `agent`, then the validator shall report `MAP-AGENT-HUMANONLY` with the map ID and step number.
- **REQ-FLOW.PERSONA.SCHEMA-004** If a map exceeds 1 MiB or 500 steps, has non-contiguous step numbers, or fails the schema, then the validator shall report a typed finding and not return a parsed map.
- **REQ-FLOW.PERSONA.SCHEMA-005** The validator shall compute the map digest over the RFC 8785 form of the map.

## Acceptance criteria

- **AC-1** Given `fixtures/flow/journey/valid-job.json` (12 steps), when validated, then no finding is reported and the digest equals `valid-job.digest`.
- **AC-2** (error path) Given `agent-humanonly.json` (step 4: actor agent, human_only true), then `MAP-AGENT-HUMANONLY` is reported for step 4.
- **AC-3** (error path) Given `gap.json` (steps 1, 2, 4), then `MAP-STEP-ORDER` is reported.
- **AC-4** (error path) Given `both.json` (a step with command and screen), then `MAP-SCHEMA` is reported.

## Design

- **Registry command(s):** none (library).
- **Data structures:** `JourneyMap`, `Step`, `Actor`, `StepTarget { Command(CommandRef) | Screen(ScreenId) }`; ordered maps only.
- **Algorithm:** bounded JSON parse (1 MiB, depth 32), schema validation, structural rules, canonical digest.
- **Errors (findings):** `MAP-SCHEMA`, `MAP-STEP-ORDER`, `MAP-AGENT-HUMANONLY`, `MAP-SIZE`.
- **Determinism:** D0.
- **Performance budget:** under 5 ms per map [unverified].
- **Crate:** `fid-accept` module `journey`.

## Test fixtures

Hand-written maps under `fixtures/flow/journey/` (project-authored).

## Build prompt

> In crate `fid-accept`, add module `journey` defining `journey-map@1` (fields: format, map_id, subject `JOB.*`, kind persona|job|setup, revision, app, phase, personas, steps[{n, actor human|operator|admin|agent|system, command `domain.verb_object@N` XOR screen, owner node ID, handoff, decision?, human_only, phase?}]). Generate and check in `schema/journey-map-v1.json` from the types. Validator: bounded parse (1 MiB, depth 32, at most 500 steps), contiguous numbering, command/screen exclusivity, `MAP-AGENT-HUMANONLY` when human_only and actor agent; RFC 8785 digest via the canonical JSON crate. Findings `MAP-SCHEMA|MAP-STEP-ORDER|MAP-AGENT-HUMANONLY|MAP-SIZE`. Tests AC-1..AC-4 of spec node FLOW.PERSONA.SCHEMA. No panic, no `unwrap`, no std HashMap.

## Open questions

- Screen IDs are free strings until UX publishes a screen inventory; the check cannot verify them yet.

## Children

_Leaf: none._
