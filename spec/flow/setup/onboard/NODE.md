---
id: FLOW.SETUP.ONBOARD
title: First tutorial hand-off in step mode with "your turn" checks
level: 3
parent: FLOW.SETUP
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-FLOW.SETUP-009]
covers: [JOB.USE.08]
depends_on: [CORE.RPL.EXEC, CORE.PRJ.TPL, CORE.JRN.CANON]
uses_artifacts: [OUTR.CONTENT.TUT, FLOW.PERSONA.PERS]
side_effect: [W2, R1]
determinism: D0
cli: [fid setup start-tutorial, fid setup check-turn]
mcp: [setup_check_turn]
journal: [setup.start_tutorial@1, setup.check_turn@1]
updated: 2026-11-17
---

# FLOW.SETUP.ONBOARD: First tutorial hand-off in step mode with "your turn" checks

## Purpose

After the sample job, hands the user into the first tutorial of their primary app: a recorded program replayed in step mode, where at each "your turn" gate the user performs the step themselves and the system checks the resulting entity state deterministically before advancing.

## Scope

**In:**
- `setup.start_tutorial@1` (W2): create a tutorial project copy and position the step cursor at the first step
- `setup.check_turn@1` (R1): compare the declared entities' state digests with the expected digests from the recorded tutorial
- Tutorial selection by primary app

**Out:**
- Tutorial content and expected digests (owned by `OUTR.CONTENT.TUT`)
- The step cursor (owned by `CORE.RPL.EXEC`); step-mode UI (owned by `VIZ`)

## Requirements

- **REQ-FLOW.SETUP.ONBOARD-001** When `setup.start_tutorial@1` runs with an `app` argument, the system shall create a project from that app's first tutorial and position the step cursor before its first step.
- **REQ-FLOW.SETUP.ONBOARD-002** At a "your turn" gate, `setup.check_turn@1` shall compute the RFC 8785 digest of each entity the gate declares and pass only if every digest equals the gate's expected digest.
- **REQ-FLOW.SETUP.ONBOARD-003** If `setup.check_turn@1` fails, then it shall name each mismatching entity and the first differing field, and the step cursor shall not advance.
- **REQ-FLOW.SETUP.ONBOARD-004** The comparison shall exclude entity IDs, timestamps and principal fields, so that a correct user action matches the recorded one.

## Acceptance criteria

- **AC-1** Given fixture tutorial `fixtures/flow/onboard/tut-plane/` (one your-turn gate: create a plane feature), when the user replays the recorded command at the gate and runs `fid setup check-turn --json`, then it passes and the cursor advances.
- **AC-2** (error path) Given the user creates the plane with a different point selection, then the check fails naming the plane and field `points`.
- **AC-3** Given the same action performed twice in fresh copies, then the digests are identical.

## Design

- **Registry command(s):** `setup.start_tutorial@1` (W2; `app: AppId`); `setup.check_turn@1` (R1; no params, uses the current tutorial project and cursor).
- **Data structures:** tutorial gate `{step, entities: Vec<EntityRef>, expected: Vec<Digest>, exclude_fields}` read from the tutorial manifest.
- **Algorithm:** canonicalise entity state minus excluded fields; SHA-256; compare; on failure diff the canonical JSON to find the first differing field.
- **Errors:** `setup.no_tutorial`, `setup.not_at_gate`.
- **Determinism:** D0.
- **Performance budget:** check under 100 ms [unverified].
- **Crate:** `fid-setup`.

## Test fixtures

A tiny synthetic tutorial under `fixtures/flow/onboard/` in the tutorial manifest format of `OUTR.CONTENT.TUT`.

## Build prompt

> In crate `fid-setup`, add `setup.start_tutorial@1` (W2; CLI `fid setup start-tutorial --app <id>`) and `setup.check_turn@1` (R1; CLI `fid setup check-turn`; MCP `setup_check_turn`). Start: create a project from the app's first tutorial and place the replay step cursor before step 1. Check: at a your-turn gate, digest each declared entity's canonical state excluding IDs, timestamps and principal fields, compare with the expected digests, report mismatching entities and the first differing field, advance the cursor only on pass. Tests AC-1..AC-3 of spec node FLOW.SETUP.ONBOARD. No panic, no `unwrap`.

## Open questions

- The tutorial manifest's gate fields must be agreed with `OUTR.CONTENT.TUT` when it is expanded.

## Children

_Leaf: none._
