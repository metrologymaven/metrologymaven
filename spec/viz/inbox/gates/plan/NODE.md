---
id: VIZ.INBOX.GATES.PLAN
title: Gate plan selection from profile, protection and principal
level: 4
parent: VIZ.INBOX.GATES
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-VIZ.INBOX.GATES-001, REQ-VIZ.INBOX.GATES-003, REQ-VIZ.INBOX.GATES-004]
covers: []
depends_on: [TRUST.PROFILE.SELECT, TRUST.PROFILE.DEFS, CORE.JRN.BR.NAME, CORE.BUS.PRIN]
uses_artifacts: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
built_in: [fid-inbox]
updated: 2026-11-22
---

# VIZ.INBOX.GATES.PLAN: Gate plan selection from profile, protection and principal

## Purpose

A pure function that, before any apply, says which human steps stand between the reviewer and CORE promotion: none, a count confirmation, a reason, a signature ceremony, or a second person. Its inputs are the active governance profile document (`sign.show_profile@1`), whether the target branch is protected, the batch size and the principal. It also defines the `CeremonyProvider` trait through which a signature ceremony is plugged in, so the Lean profile works with no signing component present: a plan that needs a ceremony which is not wired reports `ceremony_unavailable`, and apply stays disabled.

## Scope

**In:** `GatePlan` and `GateStep` types; `plan(profile, target_protected, command_count, principal) -> GatePlan`; the `CeremonyProvider` trait (registered at app start-up, optional); the rule that gates and gate steps never run for agent principals; the all-or-nothing contract: promotion is called only after every step passed.
**Out:** step UIs (`VIZ.INBOX.GATES.LEAN`, `VIZ.INBOX.GATES.REG`); calling promotion (`VIZ.INBOX.ACTIONS.CHIPS`, `VIZ.INBOX.ACTIONS.BATCH`); profile definitions and selection (`TRUST.PROFILE.DEFS`, `TRUST.PROFILE.SELECT`).

## Requirements

- **REQ-VIZ.INBOX.GATES.PLAN-001** When the target branch is protected, the plan shall include a `Signature` step, and if no `CeremonyProvider` is registered, then the plan shall be `Unavailable { reason: ceremony_unavailable }`.
- **REQ-VIZ.INBOX.GATES.PLAN-002** Under the Lean profile and an unprotected target, the plan shall be `[ConfirmCount]` for batches of more than one proposal and `[]` otherwise.
- **REQ-VIZ.INBOX.GATES.PLAN-003** Under the Regulated profile, the plan shall add a `Reason` step for every apply and a `SecondPerson` step when the profile's `program_release_by_other` is `always`.
- **REQ-VIZ.INBOX.GATES.PLAN-004** If the principal is not `human`, then the plan shall be `Unavailable { reason: human_required }` in every profile.
- **REQ-VIZ.INBOX.GATES.PLAN-005** The gate runner shall call the promotion callback only after every step reported `Passed`, and if any step is cancelled or fails, then it shall not call it and shall leave the proposal pending.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/inbox-gate-matrix.csv` (3 profiles x protected or not x batch 1 or 5 x human or agent x ceremony wired or not, with the expected plan), when `plan` runs per row, then every plan matches.
- **AC-2** Given the Lean profile, an unprotected target and no ceremony provider, then a single apply has an empty plan (launch-1 configuration).
- **AC-3** (error path) Given a fake step that cancels, then the promotion callback is never called and the proposal stays pending.
- **AC-4** Given an agent principal, then the plan is `Unavailable { human_required }`.

## Design

- **Registry command(s):** none.
- **Data structures:** `enum GateStep { ConfirmCount { n }, Reason, Signature { meanings }, SecondPerson }`; `enum GatePlan { Steps(Vec<GateStep>), Unavailable { reason } }`; `trait CeremonyProvider { fn run(&self, pre_image: EntryHash, meanings) -> Result<AssertionRef, CeremonyOutcome>; }`; `GateRunner::run(plan, ui, on_all_passed)`.
- **Algorithm:** total match on (profile name, protected, count, principal kind, provider present).
- **Errors:** none (a plan or `Unavailable`).
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-inbox` (module `gates::plan`).

## Test fixtures

`fixtures/viz/inbox-gate-matrix.csv`, hand-written from this node; CC-BY-4.0.

## Build prompt

> In crate `fid-inbox`, add module `gates::plan`: `plan(profile_doc, target_protected, command_count, principal, provider_present) -> GatePlan` with steps `ConfirmCount`, `Reason`, `Signature`, `SecondPerson` as specified (protected → Signature or `Unavailable{ceremony_unavailable}` without a provider; Lean unprotected → ConfirmCount only for batches; Regulated → Reason, plus SecondPerson when `program_release_by_other = always`; non-human → `Unavailable{human_required}`), the optional `CeremonyProvider` trait, and a `GateRunner` that calls the promotion callback only after every step passed. Read the active profile with `sign.show_profile@1`. Must compile and work with no signing crate linked. Tests AC-1..AC-4. Rules: gates never change side-effect classes and never sign on a human's behalf; no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- With no ceremony wired (launch-1, where TRUST.ESIG is out), promotion into a protected branch is impossible from the Inbox, and `CORE.JRN.BR.PROMO`'s `AssertionCheck` also has no implementation; after the first release `main` is protected (REQ-CORE.JRN.BR.NAME-006). TRUST must say how Lean produces human assertions in launch-1 [cross-domain gap].

## Children

_Leaf: none._
