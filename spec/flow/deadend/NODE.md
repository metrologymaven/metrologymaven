---
id: FLOW.DEADEND
title: No dead ends - suggested-fix catalogue and "Why?" coverage
level: 2
parent: FLOW
kind: subsystem
status: open
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
updated: 2026-11-16
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

## Open questions

- Should the catalogue live in each emitting domain's command manifest entry, with FLOW only checking it, rather than as a FLOW-owned file? Putting it in the manifest fits CONSTITUTION C3 better, but needs `CORE.REG` to add a field.

## Children

_None yet: status `open`._
