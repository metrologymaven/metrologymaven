---
id: CORE.JRN.BR.PROMO
title: Promotion and promotion requests
level: 4
parent: CORE.JRN.BR
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.JRN.BR-003, REQ-CORE.JRN.BR-004, REQ-CORE.JRN.BR-005, REQ-CORE.JRN.BR-006, REQ-CORE.JRN.BR-007]
covers: []
depends_on: [CORE.JRN.BR.NAME, CORE.JRN.BR.DIFF, CORE.JRN.CHAIN.APPEND, CORE.REG.DEF]
side_effect: [W2, W3, W1]
determinism: D0
cli: [fid journal promote-branch, fid journal promote-protected-branch, fid journal request-promote-branch]
mcp: [journal_promote_branch, journal_request_promote_branch]
journal: [journal.promote_branch@1, journal.promote_protected_branch@1, journal.request_promote_branch@1]
updated: 2026-11-17
---

# CORE.JRN.BR.PROMO: Promotion and promotion requests

## Purpose

Moves work from a source branch onto a target branch. A fast-forward moves the target head; otherwise the source's commands are re-executed on the target head by the replay engine and the resulting entries are recorded on the target with a `promoted_from` reference. Promotion into an unprotected branch is W2 and human-only; into a protected branch it is W3 and needs a valid human assertion. Agents never promote: they file a promotion request (C5).

## Scope

**In:** `journal.promote_branch@1` (W2, `human_only`), `journal.promote_protected_branch@1` (W3), `journal.request_promote_branch@1` (W1); fast-forward detection; recording re-executed entries with `promoted_from`; the `Rebaser` and `AssertionCheck` traits this crate defines and the app wires.
**Out:** re-execution itself (`CORE.RPL.RETGT` implements `Rebaser`); producing and verifying human assertions (`TRUST` implements `AssertionCheck`); generic pending requests for `human_only` commands over MCP (`CORE.BUS.POL.PROJ`); rendering requests (`VIZ.INBOX`).

## Requirements

- **REQ-CORE.JRN.BR.PROMO-001** When the target head is an ancestor of the source head, promotion shall move the target head to the source head with one `head_move` entry and re-execute nothing.
- **REQ-CORE.JRN.BR.PROMO-002** When promotion is not a fast-forward, the journal shall obtain re-executed entries from the wired `Rebaser`, append them on the target branch in order, each with `promoted_from` naming the source head, and append nothing if any step's outcome is not `IDENTICAL` or `EQUIVALENT`.
- **REQ-CORE.JRN.BR.PROMO-003** `journal.promote_branch@1` shall be side-effect class W2 with `human_only: true`, and if the target is protected, then it shall return `policy` with reason `target_protected`.
- **REQ-CORE.JRN.BR.PROMO-004** `journal.promote_protected_branch@1` shall be side-effect class W3, and if the wired `AssertionCheck` does not confirm a valid human assertion over the promotion's pre-image, then it shall return `policy` with reason `no_assertion` and commit nothing.
- **REQ-CORE.JRN.BR.PROMO-005** When `journal.request_promote_branch@1` runs, the journal shall append one W1 entry naming source head and target branch and the requester, and shall move no head.
- **REQ-CORE.JRN.BR.PROMO-006** If no `Rebaser` or no `AssertionCheck` is wired when one is needed, then the command shall return `precondition` with reason `capability_missing` and commit nothing.
- **REQ-CORE.JRN.BR.PROMO-007** If the source head moved between the request and the promotion, then the promotion shall use the source head named in the call and shall not include later entries.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/jrn/br/promo/ff` (target `main` is an ancestor of `trial`), when a human runs `fid journal promote-branch --source trial --target main --json`, then `main`'s head equals `trial`'s head, one `head_move` entry is appended, and the fake `Rebaser` is not called.
- **AC-2** Given fixture `fixtures/core/jrn/br/promo/fork` and a fake `Rebaser` returning 3 `IDENTICAL` steps, when promoted, then 3 entries are appended on `main` with `promoted_from` = the source head.
- **AC-3** (error path) Given the fork fixture and a fake `Rebaser` whose second step is `DIVERGENT`, then nothing is appended and the result names the step.
- **AC-4** (error path) Given a protected target, `promote_branch` returns `policy` / `target_protected`; `promote_protected_branch` without a confirmed assertion returns `policy` / `no_assertion`.
- **AC-5** Given an MCP agent, when it calls `journal_request_promote_branch`, then one W1 entry is appended and no head moves; when it calls `journal_promote_branch`, then a pending request is filed (projection request-only) and no head moves.
- **AC-6** (replay) Given the journal of AC-1 and AC-2, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):**
  - `journal.promote_branch@1` W2, D0, `human_only`, long-running; params `{ source: BranchRef, source_head: EntryHash, target: BranchRef }`.
  - `journal.promote_protected_branch@1` W3, D0, long-running; params as above plus `assertion: AssertionRef`.
  - `journal.request_promote_branch@1` W1, D0; params `{ source, source_head, target, note?: Untrusted<String> (≤2,000) }`.
- **Data structures:** `trait Rebaser { fn rebase(&self, source_head, target_head, cancel) -> Result<RebaseResult, _>; }` with per-step outcome; `trait AssertionCheck { fn confirm(&self, pre_image: &EntryHash, assertion: &AssertionRef) -> bool; }`; events `HeadMoved`, `PromotedEntry`, `PromotionRequested`.
- **Algorithm:** check protection and principal; if fast-forward, emit `HeadMoved`; else call `Rebaser`, require every step `IDENTICAL` or `EQUIVALENT`, emit the entries with `promoted_from`; for W3, compute the pre-image (target head plus source head plus diff digest) and call `AssertionCheck` before emitting.
- **Errors:** `policy` (`target_protected`, `no_assertion`), `precondition` (`capability_missing`, `divergent_step`), `conflict`, `cancelled`.
- **Determinism:** D0 for the decision; re-executed entries keep their own classes.
- **Performance budget:** fast-forward < 5 ms [unverified until measured].
- **Crate:** `fid-journal` (module `promote`).

## Test fixtures

`fixtures/core/jrn/br/promo/` (`ff`, `fork`), generated by a committed script; fake `Rebaser` and `AssertionCheck` in the test crate; CC-BY-4.0.

## Build prompt

> In crate `fid-journal`, add module `promote`, define traits `Rebaser` and `AssertionCheck`, and register `journal.promote_branch@1` (W2, `human_only`, long-running; refuse a protected target with `target_protected`), `journal.promote_protected_branch@1` (W3, long-running; requires `AssertionCheck::confirm` over the pre-image, else `no_assertion`) and `journal.request_promote_branch@1` (W1; records source head, target, requester and an untrusted note; moves no head). Fast-forward when the target head is an ancestor of the named source head (one `head_move` entry, no re-execution); otherwise call the wired `Rebaser` and append its entries on the target with `promoted_from`, only if every step is `IDENTICAL` or `EQUIVALENT`. Missing wiring gives `capability_missing`. CLI `fid journal promote-branch | promote-protected-branch | request-promote-branch`; MCP `journal_promote_branch` (request-only), `journal_request_promote_branch`. Tests: AC-1..AC-6 with `fixtures/core/jrn/br/promo/` and fakes. Constraints: CONSTITUTION C5 (humans adopt agent work), §3 (W3 human only, human-only commands), §8.

## Open questions

- `CORE.RPL.RETGT` (REQ-CORE.RPL.RETGT-005) implements `Rebaser`, and `TRUST` implements `AssertionCheck`; both are wired by the app at start-up. A `depends_on` would create a cycle (`CORE.RPL` depends on `CORE.JRN`; `TRUST` is a higher layer), so none is declared.
- `journal.request_promote_branch@1` overlaps with the generic pending request filed when an agent calls the `human_only` `journal.promote_branch@1` over MCP; both are kept because REQ-CORE.JRN-012 names the explicit request. `VIZ.INBOX` should show both kinds.

## Children

_Leaf: none._
