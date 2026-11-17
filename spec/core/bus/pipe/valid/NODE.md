---
id: CORE.BUS.PIPE.VALID
title: Command resolution, parameter validation and preconditions
level: 4
parent: CORE.BUS.PIPE
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.BUS.PIPE-002, REQ-CORE.BUS.PIPE-003, REQ-CORE.BUS.PIPE-004, REQ-CORE.BUS.PIPE-005]
covers: []
depends_on: [CORE.REG.DEF, CORE.REG.VER, CORE.BUS.PIPE.ERR, CORE.BUS.PIPE.HANDLER, CORE.BUS.UNIT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.BUS.PIPE.VALID: Command resolution, parameter validation and preconditions

## Purpose

The resolve, validate and precondition stages. VALID looks up `id@major` in the registry and refuses unknown, removed and (outside replay) hidden majors; validates parameters against the command's JSON Schema with the project keywords (quantity kind, unit, entity reference kind), normalising authored literals through `CORE.BUS.UNIT`; and evaluates the command's preconditions against the state view within their step bound.

## Scope

**In:** resolution and lifecycle checks; JSON Schema 2020-12 validation with `x-quantity`, `x-unit`, `x-ref`, `x-selector`; reference existence and kind checks through the state view; literal normalisation calls; precondition evaluation with the step bound; error JSON pointers.
**Out:** the schema vocabulary, quantity catalogue and precondition language (`CORE.REG.DEF`); lifecycle states (`CORE.REG.VER`); literal arithmetic (`CORE.BUS.UNIT`); authorisation (`CORE.BUS.POL.*`).

## Requirements

- **REQ-CORE.BUS.PIPE.VALID-001** If the command ID is unknown, or the major is unknown or in lifecycle state `removed`, then the bus shall return `not_found` naming the ID and major.
- **REQ-CORE.BUS.PIPE.VALID-002** If a major in lifecycle state `hidden` is invoked by any principal other than the replay engine's `system` principal, then the bus shall return `hidden_command`.
- **REQ-CORE.BUS.PIPE.VALID-003** If parameters fail the command's JSON Schema, then the bus shall return `validation` with the JSON pointer of the first failing value in document order and the failed keyword.
- **REQ-CORE.BUS.PIPE.VALID-004** If an `x-ref` parameter names an entity that does not exist in the state view or whose kind differs from the declared reference kind, then the bus shall return `validation` with reason `bad_reference`.
- **REQ-CORE.BUS.PIPE.VALID-005** When a parameter carries an authored literal, the bus shall replace it with the value normalised by `CORE.BUS.UNIT` and keep the literal for the entry.
- **REQ-CORE.BUS.PIPE.VALID-006** If a precondition evaluates false, then the bus shall return `precondition` naming the predicate.
- **REQ-CORE.BUS.PIPE.VALID-007** If a precondition evaluation exceeds its declared step bound, then the bus shall stop it and return `precondition` with reason `step_bound_exceeded`.
- **REQ-CORE.BUS.PIPE.VALID-008** If a parameter document exceeds 16 MiB or a nesting depth of 64, then the bus shall return `validation` with reason `input_limit` without parsing further.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/bus/pipe/valid/registry.json` (commands with active, deprecated, hidden and removed majors), when `nope.cmd@1`, a removed major and an unknown major are invoked, then each returns `not_found`.
- **AC-2** (error path) Given a hidden major, when invoked by a human principal, then `hidden_command`; when invoked by the replay `system` principal, then it proceeds to validation.
- **AC-3** Given fixture `fixtures/core/bus/pipe/valid/params.jsonl` (at least 60 parameter documents with expected outcomes and JSON pointers), when validated, then every outcome and pointer matches.
- **AC-4** (error path) Given an `x-ref` to a missing entity and one to an entity of the wrong kind, then both return `validation` / `bad_reference`.
- **AC-5** Given `{"value":"10.1","unit":"mm"}` for a length parameter, when validated, then the handler receives the SI value and the entry keeps the literal.
- **AC-6** (error path) Given a precondition that is false and one that loops past its step bound, then both return `precondition`, the second with `step_bound_exceeded`, and nothing is journaled.
- **AC-7** (error path) Given a 17 MiB parameter document, then `validation` / `input_limit`.

## Design

- **Registry command(s):** none; pipeline stages.
- **Data structures:** `Resolved { def: &CommandDef, lifecycle }`; `ValidationError { pointer: JsonPointer, keyword, reason }`; compiled schema cache keyed by manifest digest.
- **Algorithm:** resolve by exact `(id, major)` lookup; compile each schema once per manifest digest; validate with an allowlisted JSON Schema 2020-12 validator plus project keyword hooks; walk `x-ref` values and query the state view; call `unit::normalise` on literal objects; evaluate preconditions with the registry evaluator and its step counter.
- **Errors:** `not_found`, `hidden_command`, `validation` (reasons `schema`, `bad_reference`, `unit_mismatch`, `literal_out_of_range`, `input_limit`), `precondition` (reason `false`, `step_bound_exceeded`).
- **Determinism:** D0; document-order traversal, no clock.
- **Performance budget:** validation of a 1 KiB parameter document in < 50 µs with a warm schema cache [unverified until measured].
- **Crate:** `fid-bus` (module `validate`).

## Test fixtures

`fixtures/core/bus/pipe/valid/registry.json` and `params.jsonl`, hand-written for this project; CC-BY-4.0.

## Build prompt

> In crate `fid-bus`, add module `validate` with `resolve(id, major, principal) -> Result<Resolved, BusError>`, `validate_params(def, params, view) -> Result<ValidParams, BusError>` and `check_preconditions(def, params, view) -> Result<(), BusError>`. Unknown or removed majors give `not_found`; hidden majors give `hidden_command` unless the principal is the replay engine's system principal. Validate against the command's JSON Schema 2020-12 with an allowlisted validator plus hooks for `x-quantity`, `x-unit`, `x-ref` (entity must exist in the state view with the declared kind, else `bad_reference`) and `x-selector`; report the first failing JSON pointer in document order. Normalise authored literals with `unit::normalise` and keep the literal. Enforce 16 MiB and depth 64 before parsing deeply (`input_limit`). Evaluate preconditions with the registry's evaluator and its step bound (`step_bound_exceeded`). Tests: AC-1..AC-7 with `fixtures/core/bus/pipe/valid/`. Constraints: CONSTITUTION §8 (bounded parsers fail closed, no panic, no unsafe, deny unwrap/expect).

## Open questions

- The step bound per precondition comes from `CORE.REG.DEF` (at most 10,000 steps [unverified that this suffices]).

## Children

_Leaf: none._
