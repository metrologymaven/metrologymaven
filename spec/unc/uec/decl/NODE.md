---
id: UNC.UEC.DECL
title: Declared uncertainty command
level: 3
parent: UNC.UEC
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-UNC.UEC-007, REQ-UNC.UEC-008]
covers: []
depends_on: [UNC.UEC.STMT, UNC.UEC.CLASS, CORE]
side_effect: W2
determinism: D0
cli: [fid unc declare-uncertainty]
mcp: [unc_declare_uncertainty]
journal: [unc.declare_uncertainty@1]
updated: 2026-11-16
---

# UNC.UEC.DECL: Declared uncertainty command

## Purpose

Lets a person state an uncertainty for a characteristic (for example from a gauge's certificate or an agreed customer value) so the result is UEC-1 instead of UEC-0. The declaration is a journaled W2 command with author, source and timestamp. An agent may propose a declaration, but it lands on the agent's branch and needs human adoption before any result using it is released.

## Scope

**In:**
- `unc.declare_uncertainty@1`: parameters, validation, statement construction (method `DECLARED`, class UEC-1 through `UNC.UEC.CLASS`)
- Author, source text (untrusted string, C6), timestamp from the journal clock
- Agent-principal handling: proposal flag and the adoption requirement

**Out:**
- Branch routing and human promotion (owned by `CORE.BUS` and `CORE.JRN`)
- Release gating that refuses unadopted results (owned by `TRUST` release flow, which reads the proposal flag)
- Instrument-derived UEC-1 (owned by `UNC.INST`)

## Requirements

- **REQ-UNC.UEC.DECL-001** When a person runs `unc.declare_uncertainty@1` with a characteristic ID, expanded uncertainty U, coverage factor k (default 2), unit and source text, the system shall attach a UEC-1 statement with method `DECLARED`, the author principal and the journal timestamp, and commit one journal event.
- **REQ-UNC.UEC.DECL-002** The source text shall be stored as an untrusted string of at most 2,000 characters and never interpreted.
- **REQ-UNC.UEC.DECL-003** If U is not finite and positive, k is not finite and positive, or the unit is not a length, angle or dimensionless unit compatible with the characteristic, then the command shall fail with a typed error and commit nothing.
- **REQ-UNC.UEC.DECL-004** When the principal is an agent, the system shall set the statement's provenance to `agent_proposal` with `adopted_by` empty, so that the release gate refuses results using it until a human adopts it.
- **REQ-UNC.UEC.DECL-005** When a declaration replaces an earlier one on the same characteristic, the system shall keep the earlier statement in the journal and reference it from the new one (`supersedes`).
- **REQ-UNC.UEC.DECL-006** The declared statement shall carry a synthetic model reference `um:declared/<characteristic id>@1.0.0` whose digest is computed over the declared values, so that REQ-UNC.UEC.STMT-003 holds.

## Acceptance criteria

- **AC-1** Given project `fixtures/unc/decl/plate_hole.fidproj` with characteristic `C-001` (diameter), when `fid unc declare-uncertainty --char C-001 --U 0.004 --k 2 --unit mm --source "gauge cert 123" --json` runs as a human, then C-001 carries a UEC-1 statement with method `DECLARED`, U = 4e-6 m, author set, and exactly one `unc.declare_uncertainty@1` event is journaled.
- **AC-2** (error path) Given the same project, when the command runs with `--U -1`, then it fails with `invalid_number` and the journal head is unchanged.
- **AC-3** (error path) Given `--unit deg` on a diameter, then it fails with `unit_mismatch` and journals nothing.
- **AC-4** (agent) Given an MCP call `unc_declare_uncertainty` from an agent principal, then the event lands on the agent branch and the statement provenance is `agent_proposal` with empty `adopted_by`.
- **AC-5** (supersede) Given AC-1 followed by a second declaration with U = 0.005 mm, then the new statement's `supersedes` is the first statement's digest and both events are in the journal.
- **AC-6** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `unc.declare_uncertainty@1`, W2. Parameters: `char_id: EntityId`, `U: Decimal`, `k: Decimal = 2`, `unit: UnitCode`, `source: UntrustedString(≤2000)`, optional `coverage_p: Decimal`. Result: `{ statement_digest, class: "UEC-1", supersedes: Option<Digest> }`.
- **Data structures:** uses `UncertaintyStatement`; adds `Provenance { principal, principal_kind: Human | Agent, proposal: bool, adopted_by: Option<Principal>, supersedes: Option<Digest> }` if STMT does not already carry it.
- **Algorithm:** parse decimals; convert to SI with one correctly rounded decimal-to-binary64 parse (journaled `unit.convert`); call `uec::assign` with basis `Declared`; build the statement; commit through the bus.
- **Errors:** `invalid_number`, `unit_mismatch`, `char_not_found`, `source_too_long`.
- **Determinism:** D0 (decimal parse and record only).
- **Performance budget:** under 10 ms per declaration excluding journal fsync [unverified until measured].
- **Crate:** `fid-unc`.

## Test fixtures

`fixtures/unc/decl/plate_hole.fidproj`: a minimal synthetic project (one part, one diameter characteristic, no uncertainty), generated by a test helper, CC-BY-4.0.

## Build prompt

> In crate `fid-unc`, implement registry command `unc.declare_uncertainty@1` (side-effect class W2; CLI `fid unc declare-uncertainty`; MCP tool `unc_declare_uncertainty`; journal event `unc.declare_uncertainty@1` plus result digest). Parameters: characteristic ID, U (decimal), k (decimal, default 2), unit, source text (untrusted string, max 2,000 chars, never interpreted, CONSTITUTION C6), optional coverage probability.
> Steps: validate (finite positive U and k, unit compatible with the characteristic's quantity kind, else `invalid_number` / `unit_mismatch` / `char_not_found` / `source_too_long`, committing nothing); convert to SI metres or radians with one correctly rounded decimal parse, journaled as `unit.convert`; call `fid-kunc` `uec::assign` with basis `Declared` (gives UEC-1); build an `UncertaintyStatement` with method `DECLARED`, model ref `um:declared/<char id>@1.0.0` (digest over the declared values), author principal and journal timestamp; if a prior declaration exists set `supersedes` to its digest; commit through the command bus.
> If the principal is an agent, set provenance `agent_proposal` with empty `adopted_by` (the bus routes it to the agent branch; never adopt it yourself, CONSTITUTION C5).
> Tests (fixture `fixtures/unc/decl/plate_hole.fidproj`): happy path journals one event with U = 4e-6 m; negative U and wrong unit fail with nothing journaled; agent call lands on agent branch with proposal flag; supersede chain; exact replay is IDENTICAL. Support `--json` and `--dry-run`. No panics, no `unwrap`.

## Open questions

- None.

## Children

_Leaf: none._
