---
id: AUTO.PROG.EDIT.OPS
title: Basic step edit commands and the edit-apply core
level: 4
parent: AUTO.PROG.EDIT
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.PROG.EDIT-001, REQ-AUTO.PROG.EDIT-002, REQ-AUTO.PROG.EDIT-004]
covers: [PCD.PRG.01, PW.AUT.02]
depends_on: [AUTO.PROG.FMT, AUTO.PROG.CHECK, AUTO.REC.LIFE, AUTO.REC.ADOPT, CORE.BUS, CORE.PRJ.CONT]
side_effect: [W1, W1, W1, W1]
determinism: D0
cli: [fid program edit-insert-steps, fid program edit-delete-steps, fid program edit-move-steps, fid program edit-update-step]
mcp: [program_edit_insert_steps, program_edit_delete_steps, program_edit_move_steps, program_edit_update_step]
journal: [program.edit_insert_steps@1, program.edit_delete_steps@1, program.edit_move_steps@1, program.edit_update_step@1]
updated: 2026-11-17
---

# AUTO.PROG.EDIT.OPS: Basic step edit commands and the edit-apply core

## Purpose

The four everyday edits of a draft program (insert, delete, move, update one step's arguments or fields) as journaled W1 commands, and the shared edit-apply core that every `program.edit_*` command uses: draft-only check, optimistic concurrency on the program digest, step-ID preservation, fresh IDs from the CORE ID service for new steps, authorship records for C5, and a post-edit check whose diagnostics come back with the result. GUI editors, CLI users and agents all edit through these commands.

## Scope

**In:**
- `program.edit_insert_steps@1`, `program.edit_delete_steps@1`, `program.edit_move_steps@1`, `program.edit_update_step@1`
- `EditCore::apply(program_id, expected_digest, principal, edit_fn)`: load draft, refuse non-drafts, apply, keep IDs, assign new IDs, re-serialise canonically, record authorship, run `check_program`, journal before and after digests
- Position model: insert at `(parent_block, index)`; move preserves IDs
- Undo and redo through the journal (`CORE.JRN.UNDO`), no extra mechanism

**Out:**
- Structural refactorings (`AUTO.PROG.EDIT.REFACT`)
- View models and `program.show@1` (`AUTO.PROG.EDIT.VIEWS`)
- Lifecycle transitions (`AUTO.REC.LIFE`); adoption (`AUTO.REC.ADOPT`)

## Requirements

- **REQ-AUTO.PROG.EDIT.OPS-001** Every content change to a program shall be made only by a `program.edit_*` command executed through the bus and journaled with the program ID, the before and after program digests and the changed step IDs.
- **REQ-AUTO.PROG.EDIT.OPS-002** If the target program is not in state `draft`, then an edit command shall fail with `immutable` and change nothing.
- **REQ-AUTO.PROG.EDIT.OPS-003** If the caller's `expected_digest` differs from the program's current digest, then the edit shall fail with `stale_program` and change nothing.
- **REQ-AUTO.PROG.EDIT.OPS-004** An edit shall keep the `step_id` of every step it does not delete, and each new step shall get a UUIDv7 from the CORE ID service, recorded so replay reproduces it.
- **REQ-AUTO.PROG.EDIT.OPS-005** For every step an edit inserts or changes, the core shall call `record_authorship` with the edit's principal, so a step changed by an agent carries agent provenance until a human adopts it.
- **REQ-AUTO.PROG.EDIT.OPS-006** After applying an edit, the core shall run `check_program` and return its diagnostics with the result; check errors shall not block a draft edit.
- **REQ-AUTO.PROG.EDIT.OPS-007** If an edit would produce a document that fails `AUTO.PROG.FMT` validation (for example a step outside any block, or an `invoke` with a bad field), then it shall fail with `invalid_edit` and change nothing.

## Acceptance criteria

- **AC-1** Given fixture draft `fixtures/program/edit/base.json` (12 steps), when `fid program edit-insert-steps --program P --expected-digest <d> --at main:3 --steps @new.json --json` runs, then the program has 13 steps, the 12 old step IDs are unchanged, the new step has a fresh UUIDv7, and one journal entry holds before and after digests.
- **AC-2** Given `edit-move-steps` of steps S4 and S5 to the end, then their IDs are unchanged and their order is S4, S5 at the end.
- **AC-3** (error path) Given a registered (not draft) program, then each edit fails with `immutable`; given a wrong expected digest, `stale_program`; in both cases the journal is unchanged.
- **AC-4** Given an MCP agent session calling `program_edit_update_step` on S7, then the commit lands on the agent branch and `unadopted_steps(P)` includes S7.
- **AC-5** Given an update that introduces a type error, then the edit commits and the result contains the `type_mismatch` diagnostic.
- **AC-6** (replay) Given the journal of AC-1 and AC-2, when replayed in exact mode, then the program digest after each entry matches and the outcome is `IDENTICAL`.
- **AC-7** Given undo of AC-1 through the journal, then the program digest equals `d` again.

## Design

- **Registry command(s):** all W1 on drafts. `program.edit_insert_steps@1` (`program`, `expected_digest`, `at: Position`, `steps: Vec<StepDraft>`), `program.edit_delete_steps@1` (`program`, `expected_digest`, `steps: Vec<StepId>`), `program.edit_move_steps@1` (`program`, `expected_digest`, `steps: Vec<StepId>`, `to: Position`), `program.edit_update_step@1` (`program`, `expected_digest`, `step: StepId`, `patch: StepPatch`). Result: `{ digest, changed: Vec<StepId>, new_ids: Vec<StepId>, diagnostics }`.
- **Data structures:** `Position { block: BlockRef (main | subroutine | body of step), index: u32 }`; `StepDraft` (a step without `step_id`); `StepPatch` (field-level replacement, validated by FMT).
- **Algorithm:** load canonical draft → verify state and digest → apply pure edit function on the typed tree → assign IDs → validate with FMT → canonicalise → record authorship → check → journal.
- **Errors:** `immutable`, `stale_program`, `invalid_edit`, `unknown_step`, `bad_position`, `program_not_found`.
- **Determinism:** D0 given recorded IDs.
- **Performance budget:** one edit on a 10 000-step draft < 50 ms including check [unverified until measured].
- **Crate:** `fid-program` (module `edit::ops`).

## Test fixtures

`fixtures/program/edit/base.json` and step drafts (project-authored, CC-BY-4.0); test project from the CORE harness; fake ID service with recorded UUIDv7 values.

## Build prompt

> In crate `fid-program`, add module `edit::ops` with an `EditCore` and four registry commands, all W1 and only on drafts: `program.edit_insert_steps@1`, `program.edit_delete_steps@1`, `program.edit_move_steps@1`, `program.edit_update_step@1` (CLI `fid program edit-*`, MCP `program_edit_*`). The core loads the draft, refuses non-drafts (`immutable`, using the AUTO.REC lifecycle state) and digest mismatches (`stale_program`), applies a pure edit, keeps every surviving step ID, takes new UUIDv7s from the CORE ID service (recorded for replay), validates with FMT (`invalid_edit`), re-serialises canonically, calls `adopt::record_authorship` for inserted or changed steps with the edit's principal, runs `check_program` and returns diagnostics without blocking, and journals program ID, before/after digests and changed IDs. Undo uses the journal. Acceptance: AC-1..AC-7. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; agent edits carry provenance until a human adopts (CONSTITUTION C5).

## Open questions

- Where drafts live (project database row per program, owned by `AUTO.REC.LIFE`) is assumed; this leaf reads and writes drafts through that store.

## Children

_Leaf: none._
