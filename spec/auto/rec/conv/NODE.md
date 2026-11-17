---
id: AUTO.REC.CONV
title: Journal range to draft program
level: 3
parent: AUTO.REC
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.REC-002]
covers: [GOM.AUT.02]
depends_on: [AUTO.PROG.FMT, CORE.JRN, CORE.RPL]
side_effect: W1
determinism: D0
cli: [fid program from-journal]
mcp: [program_from_journal]
journal: [program.from_journal@1]
updated: 2026-11-16
---

# AUTO.REC.CONV: Journal range to draft program

## Purpose

`program.from_journal@1`: turns a contiguous range of a journal branch into a draft Command Program with one `invoke` step per state-changing entry, arguments stored as literals exactly as journaled. Replayed in exact mode against the range's starting state, the draft reproduces the range with outcome `IDENTICAL`. Parameter lifting and selection translation are later passes (`AUTO.REC.LIFT`) over the draft this command produces.

## Scope

**In:**
- `program.from_journal@1` (W1): branch, first and last entry hashes → new draft program
- Entry filter: W1 and W2 entries on the branch's head path in the range; R0 and R1 excluded; recording-control and program-lifecycle commands excluded
- Literal argument encoding from the journaled canonical parameters
- Provenance: each step records its source entry hash and the entry's principal (agent-authored steps keep their agent tag)
- `DraftPass` hook interface so later passes (lifting) can run on the draft

**Out:**
- Live recording marks (`AUTO.REC.MACRO`)
- Parameter lifting and selection translation (`AUTO.REC.LIFT`)
- Replay engine (`CORE.RPL`)

## Requirements

- **REQ-AUTO.REC.CONV-001** Given a contiguous range of one branch, the command shall create a draft program with one `invoke` step per W1 or W2 entry in the range, in journal order, each with the entry's command ID, major and canonical parameters as literal arguments.
- **REQ-AUTO.REC.CONV-002** The command shall exclude R0 and R1 entries, `program.record_*` entries, program and script lifecycle entries, and entries undone before the range's last entry.
- **REQ-AUTO.REC.CONV-003** Each created step shall record the source entry hash and the source principal, and steps from agent principals shall carry the agent provenance tag.
- **REQ-AUTO.REC.CONV-004** When the draft is replayed in exact mode against the state at the range's start, the replay outcome shall be `IDENTICAL` for the range.
- **REQ-AUTO.REC.CONV-005** If the range is not contiguous on one branch, or an entry's command major is no longer in the manifest, then the command shall fail with `bad_range` or `command_missing` and create nothing.

## Acceptance criteria

- **AC-1** Given fixture project `fixtures/projects/align-and-check/` whose journal holds 12 entries (8 W2, 2 R0, 1 R1, 1 undone W2), when `fid program from-journal --branch main --from <e1> --to <e12> --json` runs, then the envelope is `committed`, the draft has 7 `invoke` steps in journal order, and each step's literal arguments equal the entry's canonical parameters byte for byte.
- **AC-2** (replay) Given the draft from AC-1, when replayed with `journal.replay_exact@1` from the snapshot at `e1`, then the outcome is `IDENTICAL`.
- **AC-3** Given a range containing two agent-authored entries, then exactly those two steps carry the agent provenance tag.
- **AC-4** (error path) Given a range spanning two branches, then `bad_range`, no draft is created and the journal is unchanged.
- **AC-5** (replay) Given the journal entry of AC-1's `program.from_journal@1`, when replayed in exact mode, then the produced draft digest is identical.

## Design

- **Registry command(s):** `program.from_journal@1`, class W1 (creates a draft), params `branch: BranchRef`, `from: EntryHash`, `to: EntryHash`, `title: Option<String>`; result `{ program_id, draft_digest, step_count, skipped: Vec<{entry, reason}> }`; dry-run supported.
- **Data structures:** uses `fid-program` `Program`; `StepProvenance { source_entry, principal, agent_tag }` stored in step metadata.
- **Algorithm:** walk the head path from `from` to `to`; filter by class and command family; map each entry to `invoke` with `Arg::Literal`; assign UUIDv5 step IDs derived from (program_id, source entry hash) so the conversion is reproducible; run registered `DraftPass`es (none in this leaf).
- **Errors:** `bad_range`, `command_missing`, `range_empty`.
- **Determinism:** D0 (UUIDv5 step IDs and a program ID derived from the command's own journal entry make the draft reproducible).
- **Performance budget:** 10,000-entry range converted in < 1 s [unverified until measured].
- **Crate:** `fid-program-rec`

## Test fixtures

`fixtures/projects/align-and-check/`: a synthetic project built by a seeded generator (mesh, CAD stand-in, 12 journal entries); project-authored, CC-BY-4.0.

## Build prompt

> In a new crate `fid-program-rec`, implement registry command `program.from_journal@1` (class W1, dry-run supported; CLI `fid program from-journal`, MCP `program_from_journal`). Inputs: branch, first and last entry hashes, optional title. Walk the branch head path over the range; keep W1 and W2 entries; drop R0, R1, `program.record_*`, program and script lifecycle entries and undone entries. For each kept entry create an `invoke` step (command ID, major, each journaled canonical parameter as `Arg::Literal`), with step metadata holding the source entry hash and principal, and the agent provenance tag for agent principals. Step IDs are UUIDv5 of (program ID, source entry hash); program ID is derived from the command's own journal entry so replay reproduces the same draft. Fail with `bad_range` (not one contiguous branch range) or `command_missing` (major not in the manifest) and create nothing. Acceptance: on fixture project `align-and-check`, 7 steps with byte-identical literals; exact replay of the draft from the start snapshot is `IDENTICAL`; agent tags on exactly the agent entries; error case; replay of the command itself reproduces the digest. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; every state change through the bus (C3); the draft is W1.

## Open questions

- None.

## Children

_Leaf: none._
