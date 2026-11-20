---
id: CORE.JRN.BR.NAME
title: Branch create, list, protection and co-editing opt-in
level: 4
parent: CORE.JRN.BR
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-journal, fid-journal-cmd]
built_pr: 356
satisfies: [REQ-CORE.JRN.BR-001, REQ-CORE.JRN.BR-008]
covers: []
depends_on: [CORE.JRN.CHAIN.APPEND, CORE.JRN.CHAIN.CKPT, CORE.REG.DEF]
side_effect: [W2, R0, W3, W2]
determinism: D0
cli: [fid journal create-branch, fid journal list-branches, fid journal protect-branch, fid journal set-branch-coediting]
mcp: [journal_create_branch, journal_list_branches]
journal: [journal.create_branch@1, journal.list_branches@1, journal.protect_branch@1, journal.set_branch_coediting@1]
updated: 2026-11-17
---

# CORE.JRN.BR.NAME: Branch create, list, protection and co-editing opt-in

## Purpose

Named branches over the logical state DAG: create a branch at a head, list branches, mark a branch protected (a controlled-record change, never reversible), and record a human's opt-in that lets a named agent session commit directly to a non-agent branch. Agent branches `agent/<client>/<n>` are created through the same API by `CORE.BUS.ROUTE`.

## Scope

**In:** branch records (name, head, created-from, protected flag, co-editing opt-ins); name rules; `journal.create_branch@1`, `journal.list_branches@1`, `journal.protect_branch@1`, `journal.set_branch_coediting@1`; the `Branches::create/list/head/is_protected/coediting` API.
**Out:** diff (`CORE.JRN.BR.DIFF`); promotion (`CORE.JRN.BR.PROMO`); choosing a branch for a commit (`CORE.BUS.ROUTE`); undo head moves (`CORE.JRN.UNDO.MOVE`).

## Requirements

- **REQ-CORE.JRN.BR.NAME-001** When `journal.create_branch@1` runs with a name and a source head, the journal shall create the branch with that head and record `created_from`.
- **REQ-CORE.JRN.BR.NAME-002** If the name already exists, or does not match `[a-z0-9][a-z0-9/_.-]{0,99}`, or starts with `agent/` and the caller is not the bus's routing module, then the journal shall return `validation` with reason `bad_branch_name` and commit nothing.
- **REQ-CORE.JRN.BR.NAME-003** When `journal.list_branches@1` runs, the journal shall return every branch with name, head, created-from, protected flag and agent-or-human ownership, sorted by name.
- **REQ-CORE.JRN.BR.NAME-004** `journal.protect_branch@1` shall be side-effect class W3, shall require a `human` principal, and shall set the protected flag permanently.
- **REQ-CORE.JRN.BR.NAME-005** The journal shall offer no command or API that clears a branch's protected flag.
- **REQ-CORE.JRN.BR.NAME-006** When the first release (W3) record is committed on `main`, the journal shall mark `main` protected in the same commit.
- **REQ-CORE.JRN.BR.NAME-007** `journal.set_branch_coediting@1` shall be side-effect class W2 with `human_only: true`, and shall record or revoke an opt-in naming one branch and one agent session.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/jrn/br/name/project` (branch `main` with 5 entries), when `fid journal create-branch --name trial/a --from <head> --json` runs, then `fid journal list-branches --json` lists `main` and `trial/a` with the same head and `created_from` set.
- **AC-2** (error path) Given an existing name, `Trial A`, or `agent/x/1` from a CLI caller, then `validation` / `bad_branch_name` and nothing is journaled.
- **AC-3** Given a human, when `fid journal protect-branch main` runs, then `main` is protected; (error path) an agent invoking it over MCP gets a pending request, not a change.
- **AC-4** Given a first W3 release test command on `main`, then `main` is protected in the same entry.
- **AC-5** Given a human recording a co-editing opt-in for `main` and agent session S, then `Branches::coediting(main)` returns S; (error path) a script calling it gets `policy` / `human_only`.
- **AC-6** (replay) Given the journal of AC-1..AC-5, when replayed in exact mode, then the outcome is `IDENTICAL` and the branch list is equal.

## Design

- **Registry command(s):**
  - `journal.create_branch@1` W2, D0; params `{ name, from: EntryHash }`; result `{ branch }`.
  - `journal.list_branches@1` R0; params `{}`; result `{ branches: [BranchInfo] }`.
  - `journal.protect_branch@1` W3, D0, `not_undoable`; params `{ branch }`.
  - `journal.set_branch_coediting@1` W2, D0, `human_only`; params `{ branch, agent_session: SessionId, allow: bool }`.
- **Data structures:** `Branch { name, head, created_from, protected, coediting: BTreeSet<SessionId> }`, rebuilt from entries; events `BranchCreated`, `BranchProtected`, `CoeditingSet`.
- **Algorithm:** validate the name; emit the event; the branch table is a projection of events.
- **Errors:** `validation` (`bad_branch_name`), `not_found`, `policy` (from the bus).
- **Determinism:** D0.
- **Performance budget:** list 10,000 branches in < 20 ms [unverified until measured].
- **Crate:** `fid-journal` (module `branch`).

## Test fixtures

`fixtures/core/jrn/br/name/project`, a synthetic 5-entry project from a committed generator; CC-BY-4.0.

## Build prompt

> In crate `fid-journal`, add module `branch` and register: `journal.create_branch@1` (W2; name `[a-z0-9][a-z0-9/_.-]{0,99}`, unique, `agent/` prefix reserved for the bus routing module; records `created_from`), `journal.list_branches@1` (R0; sorted by name), `journal.protect_branch@1` (W3, human principal, not undoable, permanent) and `journal.set_branch_coediting@1` (W2, `human_only`; record or revoke an opt-in for one branch and one agent session). Expose `Branches::create/list/head/is_protected/coediting` for `CORE.BUS.ROUTE`. Protect `main` automatically in the same commit as its first W3 release record. No API clears protection. Handlers return events (`BranchCreated`, `BranchProtected`, `CoeditingSet`); the branch table is rebuilt from them. CLI `fid journal create-branch | list-branches | protect-branch | set-branch-coediting`; MCP `journal_create_branch`, `journal_list_branches`. Tests: AC-1..AC-6 with `fixtures/core/jrn/br/name/project`. Constraints: CONSTITUTION §3 (W3 human only, human-only commands D-031), C5, §8.

## Open questions

- Which commands count as "release" for REQ-006 is read from the W3 class of the committed command; confirm with `TRUST` that every release command is W3.
- Built: the branch table, name rules, events, planners and definitions in `fid_journal::branch`; the bus handlers in the new crate `fid-journal-cmd` (engine), because `fid-journal` (model) may not depend on `fid-bus`. Independent oracle: `fixtures/core/jrn/br/name/make_cases.py` writes `cases.json`, which holds the synthetic 5-entry project in place of a `project/` folder.
- Name rules are stricter than the REQ-002 pattern alone: also no empty, `.` or `..` path segment, no trailing `/`, and the name `agent` is refused, so no human branch can read as the agent namespace. An `agent` principal cannot create any branch through `journal.create_branch@1` (`validation/bad_branch_name`); agent branches come only from `plan_agent_branch`, called by `CORE.BUS.ROUTE`. There is no rename or delete, so names are never reused.
- `journal.protect_branch@1` is declared on MCP as `request_only` (AC-3 needs an agent's MCP call to become a pending request); the `mcp:` list above names only create and list.
- Deferred to unbuilt leaves: AC-1 end to end through `fid journal create-branch` and the "nothing is journaled" half of AC-2 need the commit stage (`CORE.BUS.PIPE.STAGE`) and the CLI wiring; AC-4's "in the same entry" needs the commit stage to call `release_protection`; AC-6's exact-mode replay needs `CORE.RPL.EXEC.LOOP`. The table-level halves of each are built and tested.

## Children

_Leaf: none._
