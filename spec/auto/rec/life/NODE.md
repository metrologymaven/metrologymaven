---
id: AUTO.REC.LIFE
title: Lifecycle states and content identity for programs and scripts
level: 3
parent: AUTO.REC
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
built_in: [fid-program-rec]
built_pr: 309
satisfies: [REQ-AUTO.REC-005, REQ-AUTO.REC-006, REQ-AUTO.REC-007, REQ-AUTO.REC-009]
covers: []
depends_on: [AUTO.PROG.FMT, AUTO.REC.ADOPT, CORE.BUS]
side_effect: W3
determinism: D0
cli: [fid program register, fid program unregister, fid program release, fid program deprecate, fid script register, fid script unregister, fid script release, fid script deprecate, fid program open-draft, fid script open-draft]
mcp: [program_register, program_unregister, program_release, program_deprecate, script_register, script_unregister, script_release, script_deprecate, program_open_draft, script_open_draft]
journal: [program.register@1, program.unregister@1, program.release@1, program.deprecate@1, script.register@1, script.unregister@1, script.release@1, script.deprecate@1, program.open_draft@1, script.open_draft@1]
updated: 2026-11-16
---

# AUTO.REC.LIFE: Lifecycle states and content identity for programs and scripts

## Purpose

The lifecycle state machine draft → registered → released → deprecated (plus registered → draft) for Command Programs and scripts, with content-addressed identity: a registered or released artifact is identified by the SHA-256 of its canonical form, and a released one can never change. Release and deprecation are W3, so only a human performs them and agents can only request them.

## Scope

**In:**
- Commands per artifact kind (program, script): `register` (W2), `unregister` (W2, registered → draft), `release` (W3), `deprecate` (W3)
- Transition table and refusal of all other transitions
- Version numbering per artifact lineage; digest identity
- Release precondition: program passes `program.check@1` and `AUTO.REC.ADOPT` reports no unadopted steps
- Lifecycle query API for `AUTO.RUN` and `AUTO.SCRIPTED`

**Out:**
- Adoption (`AUTO.REC.ADOPT`); signing a released artifact (`TRUST.SIGN`)
- Refusing runs of deprecated artifacts (`AUTO.RUN.BIND`, using this node's query)

## Requirements

- **REQ-AUTO.REC.LIFE-001** The system shall allow only the transitions draft → registered, registered → draft, registered → released and released → deprecated, and shall refuse any other with `illegal_transition`.
- **REQ-AUTO.REC.LIFE-002** `register` shall freeze the artifact's canonical bytes, assign the next version number in its lineage and identify it by the SHA-256 of those bytes.
- **REQ-AUTO.REC.LIFE-003** `release` and `deprecate` shall be class W3, so on MCP they file a request and only a human principal can perform them.
- **REQ-AUTO.REC.LIFE-004** If a program to be released fails `program.check@1` or has unadopted agent-authored steps, then `release` shall be refused with `check_failed` or `unadopted_steps` listing the step IDs.
- **REQ-AUTO.REC.LIFE-005** Any edit command aimed at a registered or released artifact shall be refused with `immutable`; editing starts from a new draft that copies the content and records the source digest.
- **REQ-AUTO.REC.LIFE-006** Deprecation shall not alter or remove the artifact or its run records, so earlier run records stay verifiable.

## Acceptance criteria

- **AC-1** Given fixture draft `fixtures/program/valid/minimal.json` in a test project, when a human runs `fid program register` then `fid program release --json`, then the states are registered then released, the version is 1 and the identity equals the SHA-256 of the canonical bytes.
- **AC-2** (error path) Given a released program, when `fid program register` runs on it, then `illegal_transition`; when `program.edit_insert_step@1` targets it, then `immutable`.
- **AC-3** Given an MCP agent session, when it calls `program_release`, then the result is `request_filed` and the state stays registered.
- **AC-4** (error path) Given a registered program with two unadopted agent steps, when a human releases it, then `unadopted_steps` lists both step IDs.
- **AC-5** Given a deprecated program, then its earlier run records still pass `fid-verify` (verifier fixture) and the program bytes are unchanged.
- **AC-6** Given the same sequence for a script bundle fixture `fixtures/script/hello/`, then the script commands behave identically.
- **AC-7** (replay) Given the journal of AC-1, when replayed in exact mode, then `IDENTICAL`.

## Design

- **Registry command(s):** `program.register@1` (W2), `program.unregister@1` (W2), `program.release@1` (W3), `program.deprecate@1` (W3), and the same four for `script.*`. Params: artifact ID, expected digest (conflict check), optional note; results: state, version, digest. The leaf's `side_effect: W3` names its highest class.
- **Data structures:** `Lifecycle { artifact_id, kind: Program|Script, state, version, digest, source_digest: Option<Sha256> }` stored in the project through the bus.
- **Algorithm:** transition table lookup; preconditions declared in the registry so the bus evaluates them (REQ-CORE.BUS-007).
- **Errors:** `illegal_transition`, `immutable`, `check_failed`, `unadopted_steps`, `conflict`.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-program-rec`

## Test fixtures

`fixtures/program/valid/minimal.json`; `fixtures/script/hello/` (a one-file script bundle); a test project from the CORE test harness; an agent-tagged draft `fixtures/program/agent-steps.json` (project-authored).

## Build prompt

> In crate `fid-program-rec`, implement lifecycle commands for programs and scripts: `register` (W2), `unregister` (W2), `release` (W3), `deprecate` (W3) for each of `program.*` and `script.*` (CLI `fid program|script <verb>`, MCP names by rule; W3 tools are request-only). Allowed transitions: draft→registered, registered→draft, registered→released, released→deprecated; anything else → `illegal_transition`. Register freezes canonical bytes, assigns the next lineage version and identifies by SHA-256. Release preconditions (declared in the registry, evaluated by the bus): `program.check@1` passes and `AUTO.REC.ADOPT`'s `unadopted()` is empty (`check_failed`, `unadopted_steps`). Edits to registered or released artifacts → `immutable`. Deprecation never alters bytes or run records. Expose a lifecycle query API for runs and scripted checks. Acceptance: the seven cases in the spec. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; agents never perform W3 (CONSTITUTION §3).

## Open questions

- `unregister` is added here to make the registered → draft transition of REQ-AUTO.REC-005 a named command.
- Which principal kind a human at the CLI has (and so whether `fid program release` can be performed from a terminal) is D-012.
- **Built in `fid-program-rec` (module `life`); follow-ups for unbuilt leaves:** (a) `program.check@1` (`AUTO.PROG.CHECK`) is a `ProgramChecker` seam; until it is built the host wires `CheckerUnavailable`, so every program release is `check_failed` (fail closed). (b) The bus does not evaluate preconditions yet (`CORE.BUS.PIPE.VALID`): the release preconditions are declared in the registry as FPL over the context root `artifact` (`artifact.check_passed`, `artifact.unadopted`) and the handler evaluates its own declarations with the registry evaluator. (c) `AUTO.PROG.EDIT.OPS` must call `LifeStore::guard_edit` (AC-2 `immutable`) and copy `LifeStore::frozen_bytes` into the draft that `program.open_draft@1` opens. (d) AC-5's verifier half waits for `fid-verify`; deprecation provably touches only the lifecycle state. (e) The script bundle format is open (`AUTO.PY.SDK`); a bundle's canonical form is RFC 8785 JSON of its UTF-8 files (`life::bundle`) until that node defines one. (f) MCP W3 calls file a request through a `LifeRequests` seam for the Proposal Inbox.
- **Added by the #309 review (maintainer decisions):** `program.open_draft@1` / `script.open_draft@1` (W2, branch rule, journaled, replayed) are the only way to open a new draft from a frozen version (REQ-005). `register` refuses with `conflict` when the content equals any registered, released or deprecated version of the lineage, so a deprecation cannot be undone by re-registering; queries by digest take the most restrictive state. Register events carry the frozen canonical bytes, so the journal is their durable record and replay needs no drafts.
- **Open: draft-level script provenance.** Who authored a script draft's content is not tracked yet (there is no script edit path). Registration therefore uses the version-level records `AUTO.REC.ADOPT` keeps: if any non-human authorship of the same content exists, a person who registers it is not recorded as its author and it needs `script.adopt_version@1`. Content an agent wrote outside any recorded registration and a person then registers is still recorded as the person's; closing that needs provenance recorded at script edit time (owner: the script edit path, with `AUTO.REC.ADOPT`).
- Commands added: `program.open_draft@1`, `script.open_draft@1`. Errors added to the Design list: `artifact_not_found`, `unadopted_version` (script release), `policy_refused`, `bad_params`, `journal`, `request_failed`, `internal`.

## Children

_Leaf: none._
