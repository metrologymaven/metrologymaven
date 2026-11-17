---
id: CORE.BUS.PIPE
title: Execute pipeline, handler contract and typed errors
level: 3
parent: CORE.BUS
kind: module
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-CORE.BUS-003, REQ-CORE.BUS-007, REQ-CORE.BUS-008, REQ-CORE.BUS-015]
covers: []
depends_on: [CORE.REG.DEF, CORE.REG.VER, CORE.JRN.CHAIN, CORE.JRN.STREAM, CORE.JOB.SCHED, CORE.BUS.PRIN, CORE.BUS.POL, CORE.BUS.ROUTE, CORE.BUS.UNIT]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.BUS.PIPE: Execute pipeline, handler contract and typed errors

## Purpose

The spine of `Bus::execute`. PIPE runs the fixed stage order (resolve, validate, authorise, check preconditions, route, run handler, check effects, commit, return), calls the policy, principal, routing and unit modules at the right stage, enforces the handler purity contract, hands long-running commands to the job system, and defines the typed error set every surface shares. It owns no policy itself: it guarantees that every stage runs, in order, and that a failure anywhere commits nothing.

## Scope

**In:**
- Stage order and the all-or-nothing commit guarantee
- Resolution of `id@major` against the registry, including removed and hidden majors
- Parameter validation against the registry schema (types, quantity kinds, units, reference kinds)
- Precondition evaluation using the registry's precondition evaluator
- Handler contract: immutable `StateView` in, events out; no mutable state handle reachable from a handler
- Effects check against declared event types
- Hand-off to `CORE.JOB` for commands declared long-running; commit only on success
- `BusError` with stable codes shared by all surfaces
- Overhead budget and its benchmark

**Out** (and which node owns it instead):
- Principal derivation (owned by `CORE.BUS.PRIN`)
- Class, projection, file-handle and egress policy (owned by `CORE.BUS.POL`)
- Branch choice, concurrency and idempotency (owned by `CORE.BUS.ROUTE`)
- Literal normalisation (owned by `CORE.BUS.UNIT`)
- Journal append and durability (owned by `CORE.JRN`)

## Requirements

- **REQ-CORE.BUS.PIPE-001** When a command is invoked, the bus shall run the stages resolve, validate, authorise, precondition, route, handler, effects check and commit in that order, and if any stage fails, then the bus shall commit nothing.
- **REQ-CORE.BUS.PIPE-002** If the command ID or major is unknown or `removed`, then the bus shall return a typed `not_found` error.
- **REQ-CORE.BUS.PIPE-003** If a `hidden` major is invoked by any caller other than the replay engine, then the bus shall return a typed `hidden_command` error.
- **REQ-CORE.BUS.PIPE-004** If parameters fail validation against the registry schema (type, quantity kind, unit or reference kind), then the bus shall return a typed `validation` error that names the failing JSON pointer.
- **REQ-CORE.BUS.PIPE-005** If a precondition evaluates false or exceeds its step bound, then the bus shall return a typed `precondition` error naming the predicate.
- **REQ-CORE.BUS.PIPE-006** The handler interface shall give a handler only an immutable state view and shall accept only a list of events as its output.
- **REQ-CORE.BUS.PIPE-007** If a handler returns an event type not declared in the command's effects, then the bus shall abort the commit and return a typed `undeclared_effect` error.
- **REQ-CORE.BUS.PIPE-008** When a command declared long-running is invoked, the bus shall submit it to the job system, return the job ID, and commit its events only when the job succeeds.
- **REQ-CORE.BUS.PIPE-009** Every bus error shall carry one stable code from a closed set published in the registry manifest, so every surface reports the same code.
- **REQ-CORE.BUS.PIPE-010** The bus shall add no more than 1 ms median overhead per R0 command, measured by a benchmark over 10,000 calls on reference hardware [unverified until measured].

## Interfaces

- **Offers:** `Bus::execute(command_id@major, params, channel) -> Result<CommandResult, BusError>`; `StateView`; `Handler` trait; `BusError` codes (`validation`, `policy`, `precondition`, `conflict`, `not_found`, `hidden_command`, `undeclared_effect`, `cancelled`, `unauthenticated`, `idempotency_mismatch`).
- **Consumes:** `CORE.REG` (definitions, schemas, precondition evaluator), `CORE.JRN` (append), `CORE.JOB` (submit), sibling modules PRIN, POL, ROUTE and UNIT.

## Decomposition hints

STAGE (pipeline orchestration and all-or-nothing), VALID (schema validation with project keywords), PRECOND (precondition evaluation), HANDLER (handler trait, StateView, effects check), ERR (error codes), BENCH (overhead benchmark).

## Open questions

- **Resolved by D-031 (CONSTITUTION §3 Reads):** R0 and R1 calls never reach the Record Journal; the pipeline hands a completed read to `CORE.JRN.STREAM.ALOG`, which writes it only while a recording session is active or when the call is a Command Program step. A W-class call that consumes an R1 result carries that result's digest into its entry (`CORE.BUS.PIPE.STAGE`).
- `CORE.DEP.RECOMP` needs a handler-only path for system principals (no per-element entries). `CORE.BUS.PIPE.STAGE` offers it as `Bus::run_handlers_as_system`.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| CORE.BUS.PIPE.STAGE | Stage orchestration, all-or-nothing commit, jobs, reads and overhead (leaf) | 001 008 010 | none |
| CORE.BUS.PIPE.VALID | Command resolution, parameter validation and preconditions (leaf) | 002 003 004 005 | none |
| CORE.BUS.PIPE.HANDLER | Handler contract, state view and effects check (leaf) | 006 007 | none |
| CORE.BUS.PIPE.ERR | Closed bus error code set (leaf) | 009 | none |
