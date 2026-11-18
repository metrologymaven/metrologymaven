---
id: CORE.REG.VER.LIFE
title: Lifecycle states, retention and migrator records
level: 4
parent: CORE.REG.VER
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-registry]
built_pr: 130
satisfies: [REQ-CORE.REG.VER-001, REQ-CORE.REG.VER-002, REQ-CORE.REG.VER-003, REQ-CORE.REG.VER-007]
covers: []
depends_on: [CORE.REG.DEF.FIELD]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.REG.VER.LIFE: Lifecycle states, retention and migrator records

## Purpose

The lifecycle of one (ID, major): its state, the dates and pointers that go with it, the legal transitions between states, the rule that old majors stay in the manifest until removed, and the migrator record a new major declares to translate an older major's arguments. The replay engine and the bus read these records; this leaf only defines and checks them.

## Scope

**In:** `Lifecycle` type and metadata, the transition function and its date rule, retention of non-removed majors, `MigratorRecord` and its consistency checks.
**Out:** running migrators (`CORE.RPL.EXEC.LOOP`); refusing hidden majors at run time (`CORE.BUS.PIPE`); comparing against the baseline (`CORE.REG.VER.GATE`).

## Requirements

- **REQ-CORE.REG.VER.LIFE-001** Every (ID, major) shall carry exactly one state from `active`, `deprecated`, `hidden` or `removed`, plus `since` (the release date of the state), and `replaced_by` and `remove_after` where applicable.
- **REQ-CORE.REG.VER.LIFE-002** The transition check shall allow only `active → deprecated`, `deprecated → hidden`, `hidden → removed` and `deprecated → active`, and shall reject every other transition with `illegal_transition`.
- **REQ-CORE.REG.VER.LIFE-003** If a major moves from `deprecated` to `hidden` less than 12 months after its `deprecated.since` date, then the transition check shall reject it with `deprecation_too_short` [unverified that 12 months matches the MCP ecosystem norm].
- **REQ-CORE.REG.VER.LIFE-004** The manifest input produced by this leaf shall list every (ID, major) whose state is not `removed`, including `hidden` majors, and shall list no `removed` major.
- **REQ-CORE.REG.VER.LIFE-005** A `MigratorRecord` shall hold the migrator's algorithm ID, the command ID, the source major and the target major, and the target major shall be greater than the source major.
- **REQ-CORE.REG.VER.LIFE-006** If a migrator's source major is `removed` or not declared, or two migrators share a source and target, then the check shall fail with `migrator_invalid`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/reg/ver/life/transitions.json` (all 16 state pairs with dates), when each is checked against a release date, then exactly the four legal transitions pass and the 12-month rule rejects `deprecated → hidden` at 11 months 30 days and accepts it at 12 months.
- **AC-2** Given a registry with `demo.cmd@1` hidden, `demo.cmd@2` active and `demo.old@1` removed, when the manifest input is produced, then it lists `demo.cmd@1` and `demo.cmd@2` and not `demo.old@1`.
- **AC-3** (error path) Given a migrator from `demo.cmd@3` to `demo.cmd@2`, or from a removed major, then the check fails with `migrator_invalid`.

## Design

- **Registry command(s):** none; library.
- **Data structures:** `Lifecycle { state: State, since: Date, replaced_by: Option<CommandRef>, remove_after: Option<Date> }`; `MigratorRecord { algorithm: AlgorithmId, command: CommandId, from_major: u32, to_major: u32 }`; dates are calendar dates (UTC) given by the release process, never read from a clock.
- **Algorithm:** fixed transition table; the 12-month rule compares calendar months then days.
- **Errors:** `illegal_transition`, `deprecation_too_short`, `migrator_invalid`.
- **Determinism:** D0; dates are inputs.
- **Performance budget:** negligible.
- **Crate:** `fid-registry` (module `lifecycle`).

## Test fixtures

`fixtures/core/reg/ver/life/` hand-written JSON; CC-BY-4.0.

## Build prompt

> In crate `fid-registry`, add module `lifecycle`: `Lifecycle { state: active|deprecated|hidden|removed, since, replaced_by, remove_after }`, `check_transition(old, new, release_date)` allowing only active→deprecated, deprecated→hidden (only if at least 12 calendar months after deprecated.since), hidden→removed and deprecated→active; `retained(&[CommandDecl])` returning every non-removed (ID, major) including hidden ones; `MigratorRecord { algorithm, command, from_major, to_major }` with `check_migrators` (to > from, source declared and not removed, no duplicates). Never read the wall clock; the release date is a parameter. Tests AC-1..AC-3 with fixtures in `fixtures/core/reg/ver/life/`. Constraints: CONSTITUTION §3 (major versions; old majors kept for replay), §8.

## Open questions

- The 12-month figure is the parent's [unverified] choice; it is one constant here and easy to change by decision.

## Children

_Leaf: none._
