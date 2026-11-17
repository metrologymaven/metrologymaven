---
id: FLOW.PERSONA.CHECK
title: Journey list, show and coverage check commands
level: 3
parent: FLOW.PERSONA
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-FLOW.PERSONA-004, REQ-FLOW.PERSONA-005, REQ-FLOW.PERSONA-006]
covers: []
depends_on: [FLOW.PERSONA.SCHEMA, CORE.REG.MAN, CORE.BUS.PIPE]
uses_artifacts: [FLOW.PERSONA.PERS, FLOW.PERSONA.MAPS, ECO.COMM.SCOPE]
side_effect: [R0, R0, R1]
determinism: D0
cli: [fid accept list-journeys, fid accept show-journey, fid accept check-journeys]
mcp: [accept_list_journeys, accept_show_journey, accept_check_journeys]
journal: [accept.list_journeys@1, accept.show_journey@1, accept.check_journeys@1]
updated: 2026-11-17
---

# FLOW.PERSONA.CHECK: Journey list, show and coverage check commands

## Purpose

The registry commands that list and show journey maps and the coverage check CI runs: every `JOB.*` seed row has exactly one current map, every step's owner node exists in the spec tree, every command a step names exists in the registry manifest for the phase being checked, and no human-only step is attributed to an agent.

## Scope

**In:**
- `accept.list_journeys@1` (R0), `accept.show_journey@1` (R0), `accept.check_journeys@1` (R1)
- Build-embedded inputs: all maps under `acceptance/journeys/` and the job folders, the persona file, the `JOB.*` seed ID list and the spec node-ID list
- Class-based human-only cross-check against the live manifest

**Out:**
- Structural validation (owned by `FLOW.PERSONA.SCHEMA`)
- Map content (owned by the map-owning nodes)
- The spec-tree validator that publishes the node index (owned by `ECO.COMM.SCOPE`)

## Requirements

- **REQ-FLOW.PERSONA.CHECK-001** When `fid accept check-journeys` runs, the system shall report `MAP-MISSING <id>` for every `JOB.*` seed row with no current map and `MAP-DUP <id>` for a row with more than one.
- **REQ-FLOW.PERSONA.CHECK-002** The check shall report `MAP-OWNER <map> <step>` for every step whose owner node ID is not in the embedded spec node-ID list.
- **REQ-FLOW.PERSONA.CHECK-003** For every step whose phase is at or before the `--phase` argument (default: the phase of the running build), the check shall report `MAP-COMMAND <map> <step>` if the command ID and major are absent from the live manifest; later-phase steps shall be listed as `pending`, not as failures.
- **REQ-FLOW.PERSONA.CHECK-004** If a step names a command whose manifest class is M1, S1, W3 or A0 or which is declared `human_only`, then the check shall report `MAP-HUMANONLY` unless the step has `human_only: true` and a non-agent actor.
- **REQ-FLOW.PERSONA.CHECK-005** The check shall report `MAP-UNEXERCISED <persona>` for a persona map other than `JOB.PER.06` and `JOB.PER.08` whose `exercised_by` is empty or names a job with no map.
- **REQ-FLOW.PERSONA.CHECK-006** `fid accept list-journeys` shall return map ID, subject, kind, app, revision and digest for every embedded map sorted by map ID, and `fid accept show-journey <map_id>` shall return the full map or fail with `not_found`.
- **REQ-FLOW.PERSONA.CHECK-007** The check shall exit 1 when any finding other than `pending` exists, and shall return findings sorted by code, map and step.

## Acceptance criteria

- **AC-1** Given fixture embed set `fixtures/flow/journey/complete/` (maps for every seed row of a 4-row fixture seed list) and a fixture manifest, when `fid accept check-journeys --json` runs, then it exits 0.
- **AC-2** (error path) Given the set with one map removed, then it exits 1 with `MAP-MISSING`.
- **AC-3** (error path) Given a step whose command is `sign.set_profile@1` (A0) without `human_only`, then it reports `MAP-HUMANONLY`.
- **AC-4** Given `--phase P1a` and a P2 step naming an unbuilt command, then that step is listed as `pending` and the exit code is 0.
- **AC-5** Given `fid accept show-journey nope`, then the command fails with `not_found`.

## Design

- **Registry command(s):** `accept.list_journeys@1` (R0, no params); `accept.show_journey@1` (R0, `map_id: string`); `accept.check_journeys@1` (R1, `phase?: Phase`). No file-path parameters: inputs are embedded at build time, so agents and scripts need no raw paths.
- **Data structures:** `Finding { code, map_id?, step?, detail }`.
- **Algorithm:** load embedded maps via the schema validator; set comparison with the embedded seed IDs (generated from `seeds/journeys.md` at build); node-ID lookup in the embedded list generated from the `ECO.COMM.SCOPE` index; manifest lookups for command existence, class and `human_only`.
- **Errors:** `not_found`; embedded-data load errors fail the build, not the command.
- **Determinism:** D0.
- **Performance budget:** under 1 s for 200 maps [unverified].
- **Crate:** `fid-accept`.

## Test fixtures

Fixture embed sets and manifests under `fixtures/flow/journey/` (project-authored).

## Build prompt

> In crate `fid-accept`, add commands `accept.list_journeys@1` (R0), `accept.show_journey@1` (R0, `map_id`) and `accept.check_journeys@1` (R1, optional `phase`), CLI `fid accept list-journeys|show-journey|check-journeys`, MCP `accept_list_journeys|accept_show_journey|accept_check_journeys`. Embed at build time the maps (`acceptance/journeys/**` and `acceptance/jobs/*/map.json`), the persona file, the `JOB.*` seed IDs and the spec node-ID list derived from the spec-tree index. Findings: MAP-MISSING, MAP-DUP, MAP-OWNER, MAP-COMMAND (only for steps at or before the phase; others `pending`), MAP-HUMANONLY (class M1/S1/W3/A0 or human_only without a human_only non-agent step), MAP-UNEXERCISED (except JOB.PER.06 and JOB.PER.08). Exit 1 on any non-pending finding; sorted output. Tests AC-1..AC-5 of spec node FLOW.PERSONA.CHECK. No raw path parameters; CONSTITUTION §8 rules.

## Open questions

- The node-ID list is derived from the generated index of `ECO.COMM.SCOPE`; a machine-readable node list (rather than parsing `INDEX.md`) would be a small request to ECO.

## Children

_Leaf: none._
