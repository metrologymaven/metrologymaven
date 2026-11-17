---
id: CORE.BUS.PIPE.HANDLER
title: Handler contract, state view and effects check
level: 4
parent: CORE.BUS.PIPE
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-CORE.BUS.PIPE-006, REQ-CORE.BUS.PIPE-007]
covers: []
depends_on: [CORE.REG.DEF, CORE.BUS.PIPE.ERR]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.BUS.PIPE.HANDLER: Handler contract, state view and effects check

## Purpose

Fixes what a command handler may touch. A handler gets an immutable `StateView` and returns a list of events; it holds no handle through which it could write state. After the handler returns, the effects check compares each event's type with the command's declared effects and aborts the commit on any undeclared one. The `StateView` is an abstract read interface that the project model implements, so the bus never depends on the project crate.

## Scope

**In:** the `Handler` trait, the `StateView` trait, the `Event` envelope, the effects check, compile-time absence of mutable handles.
**Out:** concrete state (`CORE.PRJ.*` implement `StateView`); declared effects lists (`CORE.REG.DEF`); the crate-graph proof that no other path to state exists (`CORE.BUS.LINT`).

## Requirements

- **REQ-CORE.BUS.PIPE.HANDLER-001** The `Handler` trait shall take only an immutable `StateView`, validated parameters and a context of principal, seed and cancel token, and shall return only a list of events or a typed handler error.
- **REQ-CORE.BUS.PIPE.HANDLER-002** The `StateView` trait shall expose read methods only, so that no handler can obtain a mutable reference to project state.
- **REQ-CORE.BUS.PIPE.HANDLER-003** If a handler returns an event whose type is not in the command's declared effects, then the bus shall abort the commit and return `undeclared_effect` naming the event type.
- **REQ-CORE.BUS.PIPE.HANDLER-004** If a handler returns more than 100,000 events or more than 256 MiB of event bodies, then the bus shall abort the commit and return `validation` with reason `effect_limit`.

## Acceptance criteria

- **AC-1** Given a compile-fail test `tests/compile_fail/mut_state.rs` in which a handler tries to take `&mut` project state, when the test suite builds, then compilation fails as expected.
- **AC-2** Given fixture `fixtures/core/bus/pipe/handler/effects.json` (a test command declaring `ElementCreated`), when its handler returns `ElementCreated`, then the effects check passes; when it also returns `ElementDeleted`, then the call fails with `undeclared_effect` naming `ElementDeleted` and nothing is journaled.
- **AC-3** (error path) Given a handler returning 100,001 events, then `validation` / `effect_limit`.

## Design

- **Registry command(s):** none.
- **Data structures:** `trait StateView { fn entity(&self, id: EntityId) -> Option<EntityRef<'_>>; fn head(&self) -> EntryHash; fn setting(&self, key) -> Option<Value>; }`; `trait Handler { fn run(&self, view: &dyn StateView, params: &ValidParams, ctx: &HandlerCtx) -> Result<Vec<Event>, HandlerError>; }`; `Event { type_id: EventType, body: CValue }`.
- **Algorithm:** effects check is a set-membership test of each `type_id` against the declared effects, in event order; first miss aborts.
- **Errors:** `undeclared_effect`, `validation` / `effect_limit`.
- **Determinism:** D0.
- **Performance budget:** effects check < 1 µs per event [unverified until measured].
- **Crate:** `fid-bus` (module `handler`).

## Test fixtures

`fixtures/core/bus/pipe/handler/effects.json` plus the compile-fail test; CC-BY-4.0.

## Build prompt

> In crate `fid-bus`, add module `handler` with `trait StateView` (read-only methods: entity lookup, head, settings), `trait Handler { run(&self, view: &dyn StateView, params, ctx) -> Result<Vec<Event>, HandlerError> }` where `ctx` holds principal, seed and cancel token, and `effects_check(def, events) -> Result<(), BusError>` returning `undeclared_effect` with the first undeclared type, and `effect_limit` above 100,000 events or 256 MiB of bodies. No method may hand out `&mut` state; add a `trybuild` compile-fail test. Tests: AC-1..AC-3 with `fixtures/core/bus/pipe/handler/effects.json`. Constraints: CONSTITUTION C3, §8 (no panic, no unsafe, deny unwrap/expect).

## Open questions

- None.

## Children

_Leaf: none._
