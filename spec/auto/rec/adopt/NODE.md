---
id: AUTO.REC.ADOPT
title: Agent-provenance gate on release
level: 3
parent: AUTO.REC
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.REC-008]
covers: []
depends_on: [AUTO.PROG.FMT, CORE.BUS, CORE.JRN]
side_effect: [W2, W2]
determinism: D0
cli: [fid program adopt-steps, fid script adopt-version]
mcp: [program_adopt_steps, script_adopt_version]
journal: [program.adopt_steps@1, script.adopt_version@1]
updated: 2026-11-17
---

# AUTO.REC.ADOPT: Agent-provenance gate on release

## Purpose

Implements C5 for programs and scripts: a human must adopt every agent-authored program step, and every agent-authored script version as a whole, before it can be released. This node owns the provenance record (who last authored each step or script version), the two human-only adoption commands (D-031: adoption commands are `human_only: true` and stay W2), and the predicates that `AUTO.REC.LIFE` uses to refuse a release with unadopted agent work.

## Scope

**In:**
- Provenance store: last author (principal kind and ID) per program step and per script version, plus the adoption that covers it
- `record_authorship(...)` API called by every code path that creates or changes steps (`AUTO.PROG.EDIT.OPS`, `AUTO.REC.CONV`, `AUTO.REC.LIFT`) and by script registration (`AUTO.REC.LIFE`)
- `program.adopt_steps@1` and `script.adopt_version@1` (W2, `human_only: true`)
- Predicates `unadopted_steps(program)` and `unadopted_script(version)`
- Loss of adoption when an agent changes an adopted step

**Out:**
- Calling `record_authorship` at edit time (`AUTO.PROG.EDIT.OPS`, `AUTO.REC.CONV`, `AUTO.REC.LIFT`)
- The release command and its refusal (`AUTO.REC.LIFE`)
- Review UI and the Proposal Inbox (`VIZ.INBOX`)
- The bus refusal of `human_only` commands for non-human principals (`CORE.BUS`)

## Requirements

- **REQ-AUTO.REC.ADOPT-001** `program.adopt_steps@1` and `script.adopt_version@1` shall be declared `human_only: true`, so the bus refuses them from agent, script, service and device principals and MCP projects them as request-only.
- **REQ-AUTO.REC.ADOPT-002** `program.adopt_steps@1` shall take the program ID, the step IDs to adopt (or `all_unadopted`) and the program digest the human reviewed, and if the current digest differs, then it shall fail with `stale_review` and adopt nothing.
- **REQ-AUTO.REC.ADOPT-003** Each adoption shall be journaled with the adopting human, the adopted step IDs (or script version digest) and the program or script digest at adoption time.
- **REQ-AUTO.REC.ADOPT-004** When `record_authorship` records an agent as the author of a step after that step was adopted, the store shall drop the adoption, so the step is unadopted again.
- **REQ-AUTO.REC.ADOPT-005** `unadopted_steps(program)` shall return, in document order, every step whose last author is an agent with no adoption recorded after that authorship.
- **REQ-AUTO.REC.ADOPT-006** `unadopted_script(version)` shall return true when the script version was registered or last changed by an agent and no `script.adopt_version@1` names that version digest.
- **REQ-AUTO.REC.ADOPT-007** If `program.adopt_steps@1` names a step that is not agent-authored or not in the program, then the command shall fail with `nothing_to_adopt` or `unknown_step` naming it and adopt nothing.

## Acceptance criteria

- **AC-1** Given fixture draft `fixtures/rec/adopt/draft-mixed.json` with steps S1 (human), S2 and S3 (agent), when a human runs `fid program adopt-steps --program P --steps S2,S3 --reviewed-digest <d> --json`, then one entry is journaled with the human, S2, S3 and digest `d`, and `unadopted_steps(P)` is empty.
- **AC-2** (error path) Given the same command from an MCP agent session and from a `script` CLI session without human upgrade, then the bus refuses both and the store is unchanged.
- **AC-3** (error path) Given `--reviewed-digest` that no longer matches (an edit happened after review), then `stale_review` and nothing adopted.
- **AC-4** Given AC-1 completed, when an agent edits S2 (`record_authorship(S2, agent)`), then `unadopted_steps(P)` returns `[S2]`.
- **AC-5** Given a script version registered by an agent, then `unadopted_script` is true; after a human runs `fid script adopt-version --script X --version-digest <v>`, it is false; a new agent-registered version is unadopted again.
- **AC-6** (error path) Given a step authored only by humans, then `nothing_to_adopt` naming it.
- **AC-7** (replay) Given the journal of AC-1, when replayed in exact mode, then the outcome is `IDENTICAL` and the store state matches.

## Design

- **Registry command(s):**
  - `program.adopt_steps@1`, W2, `human_only: true`. Params: `program: ProgramId`, `steps: StepSelection = Ids(Vec<StepId>) | AllUnadopted`, `reviewed_digest: Sha256`. Result: `{ adopted: Vec<StepId> }`.
  - `script.adopt_version@1`, W2, `human_only: true`. Params: `script: ScriptId`, `version_digest: Sha256`. Result: `{ adopted: true }`.
- **Data structures:** `Authorship { target: Step(ProgramId, StepId) | ScriptVersion(ScriptId, Sha256), principal_kind, principal_id, journal_entry }`; `Adoption { target, human_id, digest, journal_entry }`; stored in the project database as two append-only tables; current state derived by latest-entry order.
- **Algorithm:** a target is unadopted when its latest authorship is by an agent and no adoption has a later journal position.
- **Errors:** `stale_review`, `nothing_to_adopt`, `unknown_step`, `program_not_found`, `policy_refused` (from the bus).
- **Determinism:** D0.
- **Performance budget:** `unadopted_steps` on a 10 000-step program < 10 ms [unverified until measured].
- **Crate:** `fid-program-rec` (module `adopt`), with the other AUTO.REC modules.

## Test fixtures

`fixtures/rec/adopt/draft-mixed.json` and authorship records (project-authored, CC-BY-4.0); fake bus with human, script (not upgraded), and agent principals.

## Build prompt

> In crate `fid-program-rec`, add module `adopt`. Store authorship and adoption records (append-only, project database) keyed by program step or script version. Provide `record_authorship(target, principal)`, `unadopted_steps(program) -> Vec<StepId>` (document order; latest authorship by an agent with no later adoption) and `unadopted_script(version) -> bool`. Register `program.adopt_steps@1` (W2, `human_only: true`; params program, steps or `all_unadopted`, `reviewed_digest`; `stale_review` if the digest changed; `nothing_to_adopt` / `unknown_step` with nothing adopted) and `script.adopt_version@1` (W2, `human_only: true`; params script, version digest). Journal each adoption with the human, targets and digest. An agent authorship after an adoption makes the step unadopted again. CLI `fid program adopt-steps`, `fid script adopt-version`; on MCP both are request-only. Acceptance: AC-1..AC-7. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; agents never adopt (CONSTITUTION C5, §3, D-031).

## Open questions

- The target's question (which class makes a command human-only) is answered by D-031: `human_only: true` on a W2 command. A shared CORE adoption mechanism is not needed.
- `script.adopt_version@1` is new; `AUTO.REC`'s `cli`/`mcp`/`journal` lists name only `program.adopt_steps`.
- `AUTO.REC.CONV` (leaf) must call `record_authorship` for drafts it creates under an agent principal; it does not say so yet.

## Children

_Leaf: none._
