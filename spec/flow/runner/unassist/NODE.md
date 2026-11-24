---
id: FLOW.RUNNER.UNASSIST
title: Unassisted-run principal check
level: 3
parent: FLOW.RUNNER
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-FLOW.RUNNER-004]
covers: []
depends_on: [CORE.JRN.ENTRY, CORE.BUS.PRIN]
uses_artifacts: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
built_in: [fid-accept]
built_pr: 469
updated: 2026-11-22
---

# FLOW.RUNNER.UNASSIST: Unassisted-run principal check

## Purpose

A pure check over a golden journal that proves a package declared unassisted really had no agent acting in it: no Record Journal entry and no Activity Log entry attributed to an agent principal or on behalf of an agent session. A violation fails the job regardless of every other result.

## Scope

**In:**
- `check_unassisted(journal) -> Vec<Violation>` over the Record Journal and, when present, the Activity Log
- Attribution rules for session principals, upgraded sessions and agent branches

**Out:**
- Principal model and attribution fields (owned by `CORE.BUS.PRIN`, `CORE.JRN.ENTRY`)
- Combining the result into the job verdict (owned by `FLOW.RUNNER.NIGHTLY`)

## Requirements

- **REQ-FLOW.RUNNER.UNASSIST-001** The check shall report a violation for every journal entry whose principal kind is `agent`, whose session was opened by an MCP channel, or whose target is an agent branch.
- **REQ-FLOW.RUNNER.UNASSIST-002** The check shall report a violation for every Activity Log entry in the package attributed to an agent principal.
- **REQ-FLOW.RUNNER.UNASSIST-003** If an entry has no principal attribution, then the check shall report `UNATTRIBUTED` as a violation rather than assume a human.
- **REQ-FLOW.RUNNER.UNASSIST-004** Each violation shall name the entry index, command ID and principal.

## Acceptance criteria

- **AC-1** Given `fixtures/flow/unassist/human-only.jrn` (20 human and script entries), when checked, then no violation.
- **AC-2** (error path) Given `one-agent.jrn` (entry 7 by an agent), then exactly one violation at index 7.
- **AC-3** (error path) Given `unattributed.jrn`, then `UNATTRIBUTED` at the affected index.

## Design

- **Registry command(s):** none (library).
- **Data structures:** `Violation { index, stream: record|activity, command_id, principal, code }`.
- **Algorithm:** stream entries in order; inspect session header principal and channel, per-entry principal and branch.
- **Errors:** journal read errors propagate as `UNASSIST-READ`.
- **Determinism:** D0.
- **Performance budget:** 100 000 entries under 1 s [unverified].
- **Crate:** `fid-accept` module `unassist`.

## Test fixtures

Small synthetic journals under `fixtures/flow/unassist/` written with the journal test writer.

## Build prompt

> In crate `fid-accept`, add module `unassist` with `check_unassisted(journal: &JournalReader) -> Result<Vec<Violation>, UnassistError>`. Violation when an entry's principal kind is agent, its session channel is MCP, or it targets an agent branch; also for Activity Log entries by agents; `UNATTRIBUTED` for entries with no principal. Report index, stream, command ID, principal. Tests AC-1..AC-3 of spec node FLOW.RUNNER.UNASSIST. No panic or `unwrap`.

## Open questions

- None.

## Children

_Leaf: none._
