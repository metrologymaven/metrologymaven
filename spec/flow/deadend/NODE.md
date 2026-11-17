---
id: FLOW.DEADEND
title: No dead ends - suggested-fix catalogue and "Why?" coverage
level: 2
parent: FLOW
kind: subsystem
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-FLOW-006]
covers: [JOB.USE.05, JOB.USE.09]
depends_on: [CORE.REG, CORE.BUS, UNC.UEC, UNC.DEC, GDT.RES, VIZ.SHELL]
cli: [fid accept check-fixes, fid accept check-why]
mcp: [accept_check_fixes, accept_check_why]
journal: []
updated: 2026-11-17
---

# FLOW.DEADEND: No dead ends - suggested-fix catalogue and "Why?" coverage

## Purpose

Enforces the rule that no workflow ends in a state the user cannot act on. It owns the catalogue of unresolved states (NOT_DECIDABLE reasons, unresolved interpretation items, lint findings, UEC-0 reasons, blocked commands) and, for each, at least one suggested fix that names a registry command. It also owns the "Why?" coverage bar: every displayed number and every not-decidable result opens an explanation. The catalogue is checked in CI against the registry manifest, so a new reason code without a fix fails the build.

## Scope

**In:**
- The suggested-fix catalogue: reason code, owning node, one or more fixes, each naming a command ID and the arguments it needs
- The rule that every CLI and MCP result carrying such a reason includes its fixes
- The CI check that every reason code declared in the registry manifest has a catalogue entry naming existing commands
- The "Why?" coverage check over the acceptance suite's presentation tracks
- Fixes that a principal cannot run (for example an agent facing an A0 fix) shown as a request to a human, never hidden

**Out** (and which node owns it instead):
- The reason codes themselves and their semantics (owned by `UNC.DEC`, `UNC.UEC`, `GDT.RES` and each emitting domain)
- The "Why?" panel host and rendering (owned by `VIZ.SHELL`)
- Result envelopes on CLI and MCP (owned by `AUTO.CLI` and `AUTO.MCP`)
- The Shop Setup link for `rule_missing` (owned by `FLOW.SETUP`)
- Wording and visual design (owned by `UX`)

## Requirements

- **REQ-FLOW.DEADEND-001** Every reason code for a NOT_DECIDABLE outcome, an unresolved item, a lint finding or a UEC-0 result declared in the registry manifest shall have a catalogue entry with at least one suggested fix naming a registry command ID.
- **REQ-FLOW.DEADEND-002** When `fid accept check-fixes` runs, the system shall fail if any declared reason code lacks a catalogue entry or if any entry names a command ID absent from the manifest.
- **REQ-FLOW.DEADEND-003** When a command result carries a catalogued reason code, the CLI `--json` output and the MCP result shall include each suggested fix's command ID and its pre-filled arguments.
- **REQ-FLOW.DEADEND-004** If a suggested fix's side-effect class is not permitted to the current principal, then the result shall still list the fix and mark it as requiring a human, rather than omit it.
- **REQ-FLOW.DEADEND-005** When a catalogued state is shown in the GUI, the system shall offer the suggested fix as an action reachable within one interaction from the state's display.
- **REQ-FLOW.DEADEND-006** In every golden journal of the acceptance suite, every displayed value of record and every NOT_DECIDABLE result shall open a "Why?" explanation, and `fid accept check-why` shall report each display that does not.
- **REQ-FLOW.DEADEND-007** A "Why?" explanation of a standards-based result shall cite standard designator, edition and clause only, with project-authored paraphrase and no standards text (CONSTITUTION §5).

## Interfaces

- **Offers:** the suggested-fix catalogue (consumed by `AUTO.CLI`, `AUTO.MCP`, `VIZ.SHELL`, `FLOW.SETUP`); `accept.check_fixes@1` (R1); `accept.check_why@1` (R1).
- **Consumes:** reason codes from the manifest (`CORE.REG`); principal and class checks (`CORE.BUS`); `UNC.UEC`, `UNC.DEC`, `GDT.RES` reasons; the "Why?" host in `VIZ.SHELL`.

## Decomposition hints

CATALOG (fix catalogue format and entries), CHECK (manifest consistency check in CI), RESULT (fix projection into CLI and MCP results), WHY (Why? coverage check over golden journals).

## Split rationale

The dividing principle is **data, gate, runtime and GUI**: the catalogue is data (CATALOG); the CI consistency gate reads it with the manifest (CHECK); the runtime library turns entries into fixes on results for a given principal (RESULT); and the GUI-side coverage of "Why?" and fix reachability is checked on golden journals (WHY). CHECK, RESULT and WHY depend only on CATALOG, so there are no cycles.

Alternatives considered:
- **Catalogue in each command's manifest entry, FLOW only checking.** Rejected for now: it needs a new `CORE.REG.DEF` field; recorded as an open question in CATALOG.
- **One check command for fixes and "Why?".** Rejected: the fix check is a fast static manifest check that runs on every PR; the "Why?" check replays journals in a headless app and runs nightly.

## Open questions

- Resolved: the catalogue lives in a FLOW-owned file (`fid-fixes`), with per-domain fix tables referenced by ID (see `FLOW.DEADEND.CATALOG`). Moving it into the manifest stays possible later through `CORE.REG`.
- Cross-domain: `CORE.REG.DEF` needs an `x-reason-code` schema annotation so reason codes are declared in the manifest (REQ-001); `AUTO.SURF.ENV` needs an optional `suggested_fixes` field (REQ-003); `VIZ.SHELL.A11Y` and `VIZ.SHELL.TREE` need roles and attributes for the "Why?" check (REQ-005, REQ-006).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [FLOW.DEADEND.CATALOG](catalog/NODE.md) | Suggested-fix catalogue format and initial entries | 001 | JOB.USE.05 |
| [FLOW.DEADEND.CHECK](check/NODE.md) | Fix-catalogue consistency check against the registry manifest | 001 002 | JOB.USE.05 |
| [FLOW.DEADEND.RESULT](result/NODE.md) | Suggested-fix resolution per principal and result projection contract | 003 004 | JOB.USE.05 |
| [FLOW.DEADEND.WHY](why/NODE.md) | "Why?" and fix-reachability coverage check over golden journals | 005 006 007 | JOB.USE.05 JOB.USE.09 |
