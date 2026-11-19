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
built_in: [fid-bus]
built_pr: 228
satisfies: [REQ-CORE.BUS.PIPE-006, REQ-CORE.BUS.PIPE-007]
covers: []
depends_on: [CORE.REG.DEF, CORE.BUS.PIPE.ERR]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-19
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

- **AC-1** Given a compile-fail test `tests/compile_fail/mut_state.rs` in which a handler tries to take `&mut` project state, when the test suite builds, then compilation fails as expected, while a control that differs only in taking the state by shared reference builds.
- **AC-2** Given fixture `fixtures/core/bus/pipe/handler/effects.json` (a test command declaring `ElementCreated`), when its handler returns `ElementCreated`, then the effects check passes; when it also returns `ElementDeleted`, then the call fails with `undeclared_effect` naming `ElementDeleted` and nothing is journaled.
- **AC-3** (error path) Given a handler returning 100,001 events, then `validation` / `effect_limit`.

## Design

- **Registry command(s):** none.
- **Crate:** `fid-bus`, module `handler`. New dependencies: `fid-registry` (for `CommandDef::effects`) and `fid-types` (command IDs; was a dev-dependency). Both are model-layer workspace crates below `fid-bus` (engine).
- **Data structures (pinned):**
  - `trait StateView { fn entity(&self, id: &EntityId) -> Option<EntityRef<'_>>; fn head(&self) -> Digest; fn setting(&self, key: &SettingKey) -> Option<CValue>; }`. Every method takes `&self`; `EntityRef<'a> { id: &'a EntityId, kind: &'a str, body: &'a CValue }` holds shared references only. `head` is the branch head's `sha256:` `Digest` (`fid-canon`).
  - `trait Handler: Send + Sync { fn run(&self, view: &dyn StateView, params: &ValidParams, ctx: &HandlerCtx) -> Result<Vec<Event>, HandlerError>; }`. Synchronous: no handler call spans an await or a prompt.
  - `Event { type_id: EventType, body: CValue }`; envelope JSON `{"type": "...", "body": ...}`, exactly these two fields.
  - `EventType`: 1..=128 bytes, an ASCII letter first, then ASCII letters, digits, `_`, `.`, `@` (covers `ElementCreated` and registry effects such as `align.run_321@1`); compared byte for byte, case-sensitive.
  - `EntityId`: 1..=256 bytes, no control character (imported IDs are kept as text, CONSTITUTION §5). `SettingKey`: command-name grammar `[a-z0-9_.]`, alphanumeric at both ends, at most 128 bytes.
  - `ValidParams`: the validated parameter value. Its constructor `ValidParams::from_validated` is `pub(crate)`, so only the bus (the validation stage, `CORE.BUS.PIPE.VALID`, lands in `fid-bus`) builds one. `ValidParams::for_tests` (`#[doc(hidden)]`) exists only with the `test-support` cargo feature, which only `fid-bus`'s own dev-dependency on itself enables; it is not API.
  - `HandlerCtx { principal, seed: u64, cancel: CancelToken }`, all owned. Its only constructor is `HandlerCtx::derive(&SessionTable, &dyn Channel, seed, cancel)`, which calls `SessionTable::derive_principal` (`CORE.BUS.PRIN`, REQ-002) and never reads parameters. The table borrow ends when `derive` returns, so a context never holds a table borrow across a handler call, an await or a prompt.
  - `CancelSource` (owner: `cancel()`) and `CancelToken` (handler: `is_cancelled()` only), sharing one atomic flag. `fid-jobs` has its own token but depends on `fid-bus`, so the bus cannot reuse it.
  - `HandlerError = Refused(BusError) | Cancelled | Failed(Detail)`.
  - `CheckedEvents { command: CommandId, principal: Principal, events, body_bytes }`: built only by `run_handler` after the effects check, bound to the command whose definition the events were checked against and to a clone of the full `ctx.principal()`: kind, identity, `on_behalf_of`, client ID and upgraded session. A bare `PrincipalRef` would make two server-session (or internal) calls by one subject for different users attribute identically. The commit stage takes only this type, so events cannot be checked against another command's effects or attributed to another call's principal.
  - `HandlerSet`: each handler stored with its `CommandDef`, keyed by `(name, major)` taken from the definition, at most 65,536. `HandlerSet::register(def, handler)`.
- **Algorithm (pinned check order; the first failure is returned and nothing is committed):**
  1. `run_handler(set, id, view, params, ctx)` looks `id` up in the set (lookup errors below), runs the stored handler and checks against the stored definition; a `HandlerError` maps to a bus error (below).
  2. If the cancel token is set when the handler returns: `cancelled/requested`.
  3. More than 100,000 events (`MAX_EVENTS`): `validation/effect_limit`. Exactly 100,000 passes.
  4. Set-membership test of each `type_id` against `def.effects`, in event order; the first miss gives `undeclared_effect/undeclared_event_type` with detail `undeclared event type <type> at index <i>`.
  5. Every body is first scanned in full, in event order, for a non-finite number (NaN or infinity in a `Num` or `F64Bits`, which has no RFC 8785 canonical form); the first such body is `validation/schema`. Only then, in event order, is each body's canonical byte length added up. `canonical_len` counts it without building the bytes (iteratively, same rules as `fid_canon::to_canonical_bytes`, cross-checked against it in tests) and stops counting once the remaining budget is exceeded, so memory stays bounded by the events themselves. Because the scan comes first, `schema` keeps priority over `effect_limit` both within a body and across bodies. A running total above 268,435,456 bytes (256 MiB, `MAX_EVENT_BODY_BYTES`) is `validation/effect_limit` at that index. Exactly 256 MiB passes. An undeclared type is therefore reported before either, wherever it sits.
  `effects_check(def, events)` runs steps 3 to 5 alone.
- **Handler error mapping:** `Refused(e)` keeps `e` for `validation`, `policy`, `precondition`, `conflict`, `not_found`, `cancelled` and `internal`; the pipeline-only codes `hidden_command`, `undeclared_effect`, `unauthenticated`, `idempotency_mismatch` and `upgrade_refused` become `internal/handler_failed` (a handler cannot impersonate a pipeline stage). The match on `Code` has no wildcard arm, so a new code fails to build until mapped. `Cancelled` becomes `cancelled/requested`; `Failed(d)` becomes `internal/handler_failed` with detail `d`.
- **Envelope reading (`Event::from_json`):** at most 64 MiB and depth 128 (the `fid-canon` limits); too large or too deep is `validation/input_limit`; invalid JSON, a duplicate key at any depth (rejected by `fid-canon`), a missing `type` or `body`, any other field, a non-string or ill-formed `type` are `validation/schema`.
- **Handler lookup (`HandlerSet::get`):** a malformed or non-canonical ID (`x@01`) is `validation/schema`; a name bound only at other majors is `not_found/unknown_major`; any other miss is `not_found/unknown_command`. `HandlerSet::register` refuses a definition whose `id@major` is not a valid command ID, a duplicate `(name, major)` (whatever its other fields) and a full set with `internal/invariant_violated` and leaves the set unchanged: these are bus-construction mistakes.
- **Errors:** all from the frozen `bus-errors.json` v1 catalogue (`CORE.BUS.PIPE.ERR`); this leaf adds no code or reason and the catalogue digest is unchanged (`sha256:b741ab9f...`, checked by a test).
- **Compile-time absence of mutable handles (AC-1):** `tests/compile_fail/mut_state.rs` is built as a `compile_fail` doctest on `Handler` (included with `include_str!`). Stable rustdoc does not check a `compile_fail` block's error code (only nightly does, and CI runs stable), so the proof is the control: `tests/compile_fail/mut_state_control.rs`, built as a passing doctest, is the same file with the view taken by shared reference, and a test asserts the two files differ only in that `&mut`. A further `compile_fail` doctest shows a `&dyn StateView` cannot become `&mut dyn StateView`. A doctest needs no `trybuild` dependency.
- **Bounds:** events, body bytes, envelope size and depth, ID lengths and handler count are all capped; no code path panics, unwraps or uses `unsafe`.
- **Determinism:** D0. The check is a pure function of the definition and the events.
- **Performance budget:** effects check < 1 µs per event [unverified until measured]; membership is a `BTreeSet<&str>` lookup.

## Test fixtures

`fixtures/core/bus/pipe/handler/effects.json` (AC-2, hand-labelled) and `fixtures/core/bus/pipe/handler/envelopes.json` (envelope refusals, hand-labelled), plus the compile-fail test `crates/fid-bus/tests/compile_fail/mut_state.rs` and its control `mut_state_control.rs`; CC-BY-4.0. Tests: `crates/fid-bus/tests/handler.rs`.

## Build prompt

> In crate `fid-bus`, add module `handler` with `trait StateView` (read-only methods: entity lookup, head, settings), `trait Handler { run(&self, view: &dyn StateView, params, ctx) -> Result<Vec<Event>, HandlerError> }` where `ctx` holds principal, seed and cancel token, and `effects_check(def, events) -> Result<(), BusError>` returning `undeclared_effect` with the first undeclared type, and `effect_limit` above 100,000 events or 256 MiB of bodies. No method may hand out `&mut` state; add a `trybuild` compile-fail test. Tests: AC-1..AC-3 with `fixtures/core/bus/pipe/handler/effects.json`. Constraints: CONSTITUTION C3, §8 (no panic, no unsafe, deny unwrap/expect).

## Open questions

- None.

## Children

_Leaf: none._
