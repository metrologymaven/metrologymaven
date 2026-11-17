---
id: CORE.JRN.BR
title: Branches, diff, protection and promotion
level: 3
parent: CORE.JRN
kind: module
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-CORE.JRN-010, REQ-CORE.JRN-011, REQ-CORE.JRN-012]
covers: []
depends_on: [CORE.JRN.CHAIN, CORE.REG.DEF]
cli: [fid journal create-branch, fid journal list-branches, fid journal diff-branch, fid journal promote-branch, fid journal promote-protected-branch, fid journal request-promote-branch, fid journal protect-branch, fid journal set-branch-coediting]
mcp: [journal_create_branch, journal_list_branches, journal_diff_branch, journal_promote_branch, journal_request_promote_branch]
journal: [journal.create_branch@1, journal.promote_branch@1, journal.promote_protected_branch@1, journal.request_promote_branch@1, journal.protect_branch@1, journal.set_branch_coediting@1]
updated: 2026-11-17
---

# CORE.JRN.BR: Branches, diff, protection and promotion

## Purpose

The logical side of the journal: named branches over the state DAG, entity-by-entity diff between branches, protected branches, and promotion. Agents work on their own branches and may only *request* promotion; humans promote. Promotion into a protected branch is a controlled-record change (W3) with a human assertion.

## Scope

**In:**
- Branch create and list; branch names including `agent/<client>/<n>`
- Logical DAG via `logical_parent`
- Diff between two branch heads keyed by stable entity ID, including result output digests
- Protection flag (setting it is W3; `main` becomes protected at first release)
- Promotion: fast-forward when possible; otherwise record the result of re-executing the branch on the target head, citing `promoted_from`
- Separate commands for unprotected (W2) and protected (W3) promotion, because one command has one class
- Promotion request records for agents

**Out** (and which node owns it instead):
- Choosing the branch a commit goes to (owned by `CORE.BUS.ROUTE`)
- Re-executing branch commands onto a moved head (owned by `CORE.RPL.RETGT`)
- Rendering proposals and the four-layer diff UI (owned by `VIZ`)
- Producing human assertions (owned by `TRUST`)

## Requirements

- **REQ-CORE.JRN.BR-001** The journal shall support named branches, each with a head on the logical DAG, and shall reject a branch name that already exists.
- **REQ-CORE.JRN.BR-002** When `journal.diff_branch@1` compares two branches, the journal shall list added, removed and changed entities keyed by stable entity ID, with old and new output digests for each changed entity.
- **REQ-CORE.JRN.BR-003** When the target head is an ancestor of the source head, promotion shall fast-forward the target head without re-executing commands.
- **REQ-CORE.JRN.BR-004** When promotion is not a fast-forward, the journal shall record the re-executed entries on the target branch with a `promoted_from` reference to the source head.
- **REQ-CORE.JRN.BR-005** `journal.promote_branch@1` shall be side-effect class W2, shall require a human principal, and shall refuse a protected target.
- **REQ-CORE.JRN.BR-006** `journal.promote_protected_branch@1` shall be side-effect class W3 and shall commit only with a human principal and a valid human assertion.
- **REQ-CORE.JRN.BR-007** When an agent principal invokes `journal.request_promote_branch@1`, the journal shall append a W1 promotion-request entry naming source and target, and shall not move any head.
- **REQ-CORE.JRN.BR-008** Marking a branch protected shall be a W3 command, and no command shall remove protection.

## Interfaces

- **Offers:** `Branches::create/list/head/diff/promote/request_promote/protect`; the diff result type consumed by `VIZ.INBOX` as the semantic and result layers of its proposal diff.
- **Commands:** `journal.create_branch@1` (W2), `journal.list_branches@1` (R0), `journal.diff_branch@1` (R0), `journal.promote_branch@1` (W2), `journal.promote_protected_branch@1` (W3), `journal.request_promote_branch@1` (W1), `journal.protect_branch@1` (W3).
- **Consumes:** `CORE.JRN.CHAIN`.

## Decomposition hints

NAME (create, list, naming rules), DIFF (entity and result diff), PROMO (fast-forward and recorded re-execution), PROT (protection and W3 promotion), REQ (agent promotion requests).

## Open questions

- `VIZ.INBOX` asks who computes the "result diff"; this child offers output-digest diff per entity, and value-level deltas would come from `CORE.RPL.XVER`-style comparison. Confirm with VIZ.
- The parent's interface listed one promote command at W2 or W3; this child splits it in two because a registry command has exactly one class.
- Non-fast-forward promotion needs `CORE.RPL.RETGT`'s rebase, but `CORE.RPL` depends on `CORE.JRN`, so a code edge would form a cycle. `CORE.JRN.BR.PROMO` defines a `Rebaser` trait that the replay crate implements and the app wires at start-up; no `depends_on` is declared.
- `journal.set_branch_coediting@1` (W2, `human_only`) is added in `CORE.JRN.BR.NAME` to record the human opt-in that `CORE.BUS.ROUTE` reads (REQ-CORE.BUS-006).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| CORE.JRN.BR.NAME | Branch create, list, protection and co-editing opt-in (leaf) | 001 008 | none |
| CORE.JRN.BR.DIFF | Entity and result diff between branches (leaf) | 002 | none |
| CORE.JRN.BR.PROMO | Promotion and promotion requests (leaf) | 003 004 005 006 007 | none |
