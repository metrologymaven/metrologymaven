---
id: TRUST.PROFILE.SELECT
title: Set and show the active governance profile
level: 3
parent: TRUST.PROFILE
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-profile-cmd]
built_pr: 378
satisfies: [REQ-TRUST.PROFILE-002]
covers: []
depends_on: [TRUST.PROFILE.DEFS, CORE.REG.DEF, CORE.BUS.PIPE.HANDLER, CORE.BUS.POL.CLASS, CORE.STO.SET]
side_effect: A0
determinism: D0
cli: [fid sign set-profile, fid sign show-profile]
mcp: [sign_show_profile]
journal: [sign.set_profile@1]
updated: 2026-11-17
---

# TRUST.PROFILE.SELECT: Set and show the active governance profile

## Purpose

The two commands that choose and display the tenant's active profile: `sign.set_profile@1` (A0, never on MCP) stores the instantiated profile document in the tenant configuration and journals old and new digests; `sign.show_profile@1` (R0) returns the active document and digest. The leaf's side-effect class is that of the mutating command.

## Scope

**In:**
- `sign.set_profile@1` (A0): name plus optional tenant parameter file
- `sign.show_profile@1` (R0)
- Persisting the active profile in tenant configuration through CORE storage
- Journal event with old and new digests
- Default when nothing is set (Lean)

**Out:**
- Profile definitions (`TRUST.PROFILE.DEFS`)
- Effects of the active profile (`TRUST.PROFILE.GATE`)

## Requirements

- **REQ-TRUST.PROFILE.SELECT-001** `sign.set_profile@1` shall be class A0, shall have no MCP projection, and shall be refused with a typed policy error for any non-human principal.
- **REQ-TRUST.PROFILE.SELECT-002** When `sign.set_profile@1` succeeds, the system shall append one `sign.set_profile@1` journal event holding the old digest, the new digest and the profile name.
- **REQ-TRUST.PROFILE.SELECT-003** If the new instantiated profile has the same digest as the active one, then the command shall succeed without a journal event and report `unchanged`.
- **REQ-TRUST.PROFILE.SELECT-004** When no profile has been set, `sign.show_profile@1` shall report the Lean built-in with origin `default`.
- **REQ-TRUST.PROFILE.SELECT-005** `sign.show_profile@1` shall be class R0 and return the profile name, the instantiated document and its digest.

## Acceptance criteria

- **AC-1** Given a fresh tenant from fixture `fixtures/projects/plate-3holes`, when `fid sign show-profile --json` runs, then the profile is `lean` with origin `default` and the pinned Lean digest.
- **AC-2** Given a human principal at an attended session, when `fid sign set-profile regulated --params fixtures/profile/regulated-with-xqa.json` runs, then `show-profile` reports the new digest and one journal event holds old (Lean) and new digests.
- **AC-3** (error path) Given an agent principal, when the command is invoked through any surface, then it fails with the A0 policy error and nothing is journaled; and `tools/list` on `fid mcp` shows no `sign_set_profile`.
- **AC-4** Given AC-2 run a second time, then the result is `unchanged` and no new journal entry exists.
- **AC-5** (replay) Given the journal from AC-2, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `sign.set_profile@1`, A0, params `name: ProfileName`, `params: Option<FileHandle>`; result `{ old_digest, new_digest, status: changed | unchanged }`. `sign.show_profile@1`, R0, no params; result `{ name, origin, document, digest }`.
- **Data structures:** tenant config key `trust.profile` holding the instantiated document.
- **Algorithm:** instantiate via `fid-profile::defs`; compare digests; write config through CORE storage in the same atomic commit as the journal event.
- **Errors:** passthrough `profile.*`; CORE policy error for non-human principals.
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-profile` (module `select`), registered with the command registry.

## Test fixtures

- `fixtures/projects/plate-3holes`, `fixtures/profile/*`.

## Build prompt

> In crate `fid-profile`, add module `select` with registry commands `sign.set_profile@1` (A0; CLI `fid sign set-profile <name> [--params F]`; never projected to MCP) and `sign.show_profile@1` (R0; CLI `fid sign show-profile`; MCP `sign_show_profile`). Set: instantiate with `fid-profile::defs`, refuse non-human principals (bus policy error), store under tenant config key `trust.profile` atomically with one journal event `{old_digest, new_digest, name}`; same digest gives `unchanged` with no event. Show: return name, origin (`default` when unset, Lean), document and digest. Tests: the five acceptance criteria. Constraints: CONSTITUTION §3 (A0 never on MCP, agents never perform A0), §8, D0.

## As built (fid-profile-cmd, #378; maintainer decisions on review)

- **Deviation: only `main` counts.** The active profile is folded from entries on `main` only; a `sign.set_profile@1` entry on any other branch is ignored, and a promoted copy (`promoted_from` set) is refused. `set_profile` must run directly on `main`: the handler refuses a call that would commit elsewhere with `precondition/false`. The fold takes the branch and adding stage from the journal's entry origin (`fid_journal::branch::EntryOrigin`), never from the event.
- **Direct human action only.** A `ProfileSet` event in an entry that also carries any `Request*` event (an approved agent request) is refused.
- **Decision binding.** `ProfileSet` carries `decided_at`: the entry hash of the last applied `ProfileSet` entry, or `genesis`. The handler takes it from the state it read; the fold refuses a mismatch. The commit stage must compare-and-swap on that value atomically with the commit and refuse a stale set with `conflict/head_moved`, so two concurrent sets cannot both commit and an old payload replayed later is refused.
- **Fail closed.** A committed entry that the fold refuses means the commit stage has a bug (it let through what the compare-and-swap and the checks must stop); the fold returns `internal/invariant_violated` and the folded state is not trusted past it.
- **`trust.profile` is not a setting.** The configuration key is the folded state of the journal; it must never be declared as a `CORE.STO.SET` setting (no reserved-keys list exists in CORE.STO.SET today to record it in).

## Open questions

- REQ-TRUST.PROFILE.SELECT-001 needs a human principal; with D-012 open, a CLI caller is `script` today, so `set-profile` works only from the GUI admin screen until D-012 is decided.

## Children

_Leaf: none._
