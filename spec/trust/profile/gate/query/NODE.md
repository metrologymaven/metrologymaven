---
id: TRUST.PROFILE.GATE.QUERY
title: Read-only profile query API and GUI gating flags
level: 4
parent: TRUST.PROFILE.GATE
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-TRUST.PROFILE.GATE-005]
covers: []
depends_on: [TRUST.PROFILE.DEFS, TRUST.PROFILE.SELECT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
built_in: [fid-profile]
built_pr: 486
updated: 2026-11-22
---

# TRUST.PROFILE.GATE.QUERY: Read-only profile query API and GUI gating flags

## Purpose

The one read path to the active governance profile: `ProfileQuery` returns the active instantiated document, its digest and typed accessors, and derives the GUI gating flags (which controls to hide, disable or require). Every other gate leaf, the exporter and the signing ceremony read the profile through it, so answers are consistent.

## Scope

**In:**
- `ProfileQuery` trait and its implementation over the stored selection
- Typed accessors for every DEFS parameter
- `GuiGates` flags derived from a profile document
- A snapshot type so one command sees one profile throughout

**Out:**
- Profile definitions and digest (`TRUST.PROFILE.DEFS`)
- Changing the active profile (`TRUST.PROFILE.SELECT`)
- Enforcement hooks (`TRUST.PROFILE.GATE.ASSIST`, `TRUST.PROFILE.GATE.RELEASE`)

## Requirements

- **REQ-TRUST.PROFILE.GATE.QUERY-001** `ProfileQuery` shall expose only read methods: `active() -> ProfileSnapshot`, and on a snapshot `name()`, `digest()`, `document()` and one typed accessor per DEFS parameter.
- **REQ-TRUST.PROFILE.GATE.QUERY-002** For the same profile digest, every accessor and `gui_gates()` shall return equal values on every call and every platform.
- **REQ-TRUST.PROFILE.GATE.QUERY-003** When no profile has been set, `active()` shall return the Lean built-in snapshot, consistent with `sign.show_profile@1`.
- **REQ-TRUST.PROFILE.GATE.QUERY-004** `gui_gates()` shall return, for Regulated, `require_signature_before_release: true` and `software_key_signing: disabled`; for Assistive-only, `agent_direct_write: disabled`; and for Lean, all flags permissive.
- **REQ-TRUST.PROFILE.GATE.QUERY-005** A snapshot taken at the start of a command shall stay unchanged for that command even if the active profile is changed concurrently.

## Acceptance criteria

- **AC-1** Given each built-in from `fixtures/profile/builtin/`, when `active()` is read after selecting it, then name, digest and all accessors equal the fixture values, and `gui_gates()` equals `fixtures/profile/gates/<name>.json`.
- **AC-2** Given no selection, then `active()` is Lean with the pinned Lean digest.
- **AC-3** Given a snapshot held while another thread sets Regulated, then the held snapshot still reports Lean and a new `active()` reports Regulated.
- **AC-4** (API) A compile-fail test shows no `&mut` method is reachable from `ProfileQuery` or `ProfileSnapshot`.

## Design

- **Registry command(s):** none (library; `sign.show_profile@1` in SELECT uses it).
- **Data structures:** `ProfileSnapshot(Arc<ProfileDoc>, Digest)`; `GuiGates { require_signature_before_release, software_key_signing: Enabled|Disabled, agent_direct_write: Enabled|Disabled, required_meanings: Vec<MeaningCode> }`.
- **Algorithm:** read the stored selection (SELECT's tenant config key `trust.profile`), instantiate via DEFS, cache by digest; derive gates by a fixed table.
- **Errors:** passthrough `profile.malformed` if the stored selection fails to instantiate (then the GUI shows the error and gates fail closed to Regulated flags).
- **Determinism:** D0.
- **Performance budget:** `active()` < 1 µs from cache [unverified until measured].
- **Crate:** `fid-profile`, module `query`.

## Test fixtures

`fixtures/profile/builtin/*.json` (from DEFS) and new `fixtures/profile/gates/{lean,regulated,assistive_only}.json`.

## Build prompt

> In crate `fid-profile`, add module `query` with trait `ProfileQuery { fn active(&self) -> ProfileSnapshot; }` and `ProfileSnapshot` (immutable, `Arc`-shared) exposing `name()`, `digest()`, `document()`, one typed accessor per `Params` field, and `gui_gates() -> GuiGates`. Implement it over the stored selection written by module `select` (tenant config key `trust.profile`); unset means Lean. Gate table: Regulated → `require_signature_before_release: true`, `software_key_signing: Disabled`; Assistive-only → `agent_direct_write: Disabled`; Lean → permissive. If the stored selection fails to instantiate, return the error and make gates fail closed (Regulated flags). Snapshots never change after creation. Tests: AC-1..AC-4. Constraints: CONSTITUTION §8, D0, read-only (no `&mut` API).

## Open questions

- Fail-closed to Regulated flags on a corrupt selection is a proposal; maintainers may prefer refusing to start.

## Children

_Leaf: none._
