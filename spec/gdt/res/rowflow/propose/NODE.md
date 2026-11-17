---
id: GDT.RES.ROWFLOW.PROPOSE
title: Propose a DMT row revision
level: 4
parent: GDT.RES.ROWFLOW
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.RES.ROWFLOW-001, REQ-GDT.RES.ROWFLOW-003]
covers: []
depends_on: [GDT.RES.ROWFLOW.STATE, GDT.RES.SCHEMA, GDT.RES.LINT, CORE.JRN.BR, CORE.BUS]
side_effect: W1
determinism: D0
cli: [fid gdt propose-dmt-row]
mcp: [gdt_propose_dmt_row]
journal: [gdt.propose_dmt_row@1]
updated: 2026-11-17
---

# GDT.RES.ROWFLOW.PROPOSE: Propose a DMT row revision

## Purpose

The entry point for any change to a DMT row: a new row, a revision, or a human taking over an agent's draft. Agents may call it (on their own branch, provenance-tagged) so they can draft structure, but an agent draft can never be reviewed until a human re-proposes it as its author. Each call creates one new revision through the state machine and runs the draft-mode lint so obviously bad rows (long free text, malformed clause numbers) are refused at once.

## Scope

**In:** `gdt.propose_dmt_row@1` (W1); params: row content (schema `DmtRow` minus review state), optional `derived_from` revision; draft-mode lint; new `RowRevision` in `draft`; routing of agent commits to the agent branch (by `CORE.BUS`).
**Out:** review and adoption (owned by `GDT.RES.ROWFLOW.REVIEW`, `GDT.RES.ROWFLOW.ADOPT`); lint rules (owned by `GDT.RES.LINT`); interpretation content (expert work in `GDT.RES.ROWASME`, `GDT.RES.ROWISO`).

## Requirements

- **REQ-GDT.RES.ROWFLOW.PROPOSE-001** When an agent principal proposes a row, the command shall commit a `draft` revision with `author_kind: agent` and `provenance: agent` on the agent's branch, never on a human branch.
- **REQ-GDT.RES.ROWFLOW.PROPOSE-002** When a human principal proposes with `derived_from` naming an agent revision, the new revision shall have the human as author and keep the link to the agent revision.
- **REQ-GDT.RES.ROWFLOW.PROPOSE-003** If the proposed content fails schema parsing or any draft-mode lint rule (L1, L2), then the command shall fail with the lint finding and journal nothing.
- **REQ-GDT.RES.ROWFLOW.PROPOSE-004** If the proposed content digest equals the latest revision's digest for that key, then the command shall fail with `no_change` and journal nothing.
- **REQ-GDT.RES.ROWFLOW.PROPOSE-005** A committed proposal shall journal one `gdt.propose_dmt_row@1` event with the key, revision number, content digest, author and provenance.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/rowflow/propose_agent.json` and an MCP (agent) channel, when `gdt_propose_dmt_row` is called, then a `draft` revision with agent provenance is committed on `agent/<client>/<n>` and one journal event is appended.
- **AC-2** Given a human CLI session upgraded to human (D-012) and `--derived-from <agent revision>`, when `fid gdt propose-dmt-row --json` runs, then the revision's author is the human and `derived_from` is set.
- **AC-3** (error path) Given a row with a 300-character limitation note, then the L1 finding is returned and nothing is journaled.
- **AC-4** (error path) Given identical content to the latest revision, then `no_change`.
- **AC-5** (replay) Exact replay of AC-1 gives `IDENTICAL`.

## Design

- **Registry command(s):** `gdt.propose_dmt_row@1`, W1 (agent branch allowed). Params: `row: DmtRowDraft`, `derived_from: Option<RevisionId>`. Result: `{ key, revision, content_digest }`. `--json`, `--dry-run` (runs parse and lint only).
- **Data structures:** `DmtRowDraft` (schema `DmtRow` without review state).
- **Algorithm:** parse → draft lint (L1, L2) → digest → compare with latest → `state::transition(Propose)` → commit.
- **Errors:** schema errors, lint findings, `no_change`, `unknown_revision`.
- **Determinism:** D0.
- **Performance budget:** < 20 ms per proposal [unverified].
- **Crate:** `fid-gdt` (module `dmt_flow::cmd`).

## Test fixtures

Synthetic rows only (invented clause numbers, lorem-ipsum notes); CC0.

## Build prompt

> In crate `fid-gdt`, module `dmt_flow::cmd`, implement registry command `gdt.propose_dmt_row@1` (W1; CLI `fid gdt propose-dmt-row`; MCP `gdt_propose_dmt_row`; one journal event; `--json`, `--dry-run`). Params `row: DmtRowDraft`, optional `derived_from`. Steps: parse with the `fid-gps` DMT schema; run lint rules L1 and L2 in draft mode (fail with the finding, nothing journaled); compute the content digest; `no_change` if equal to the latest revision of the key; apply `dmt_flow::state::transition(Propose{author, author_kind, derived_from})`; commit (the bus routes agent principals to their branch). Agent proposals carry `author_kind: agent` and `provenance: agent`; a human re-proposal with `derived_from` takes authorship. Journal key, revision, content digest, author, provenance. Tests per the acceptance criteria (agent branch, human takeover, lint failure, no-change, exact replay). Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), C5 (agent work is provenance-tagged), never include standards text in code, comments or fixtures.

## Open questions

- None.

## Children

_Leaf: none._
