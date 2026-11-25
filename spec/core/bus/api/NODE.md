---
id: CORE.BUS.API
title: Bus API crate: the call types and port trait surfaces see
level: 3
parent: CORE.BUS
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-bus-api, fid-types]
satisfies: [REQ-CORE.BUS-001, REQ-CORE.BUS-016]
covers: []
depends_on: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-25
---

# CORE.BUS.API: Bus API crate: the call types and port trait surfaces see

## Purpose

`fid-bus-api` is the one CORE crate a surface crate may link (REQ-CORE.BUS.LINT-004, `bus_api` in `lint-allowlist.toml`). Until now it existed only as a name. This leaf defines it as a thin crate of types and one trait: the CORE form of one call (`BusCall`), its reply (`ApiReply`, `ApiError`), the principal kind a session holds, and `BusPort`, the trait that an in-process bus session implements. The crate holds no bus, no session table and no presence provider. It cannot run a command or make anyone `human`. It only names what a surface may ask for. The implementation lives in `fid-bus` (`CORE.BUS.PORT`), and a composition root above every domain hands surfaces ready-made ports (`AUTO.SURF.COMPOSE`). Because the crate depends on nothing but `fid-canon` and `fid-types`, `fid-surface` can link it and still build for wasm32, and linking it gives a surface no route to state.

## Scope

**In:**
- Crate `fid-bus-api`: its dependencies, its layer in `layers.toml` and its role as the `bus_api` door of `CORE.BUS.LINT`
- `BusCall`: command ID, parameter document, dry-run flag
- `ApiReply` and `ApiError`: success forms, and the bus error's code, reason, detail and the stage that raised it
- `PrincipalKind` with the six wire strings of `CORE.BUS.PRIN`, added to `fid-types`
- `PortKind` and `ChannelInfo`: the read-only description of the channel a port sends on
- The `BusPort` trait: `channel`, `principal_kind`, `confirm_human`, `call`

**Out:**
- Implementing `BusPort` over `fid_bus::Bus`, sessions, upgrade grants and the stage report (`CORE.BUS.PORT`)
- `BusRequest`, `BusEvent`, `BusTransport` and the invocation tracker, which stay in `fid-surface` (`AUTO.SURF.JOBS`)
- Envelopes and the translation of an `ApiError` into one (`AUTO.SURF.ENV`, `AUTO.SURF.INPROC`)
- Which handlers a bus holds and who builds ports (`AUTO.SURF.COMPOSE`)
- The bus error catalogue itself (`CORE.BUS.PIPE.ERR`) and the crate-graph check (`CORE.BUS.LINT`)

## Requirements

- **REQ-CORE.BUS.API-001** The bus API crate `fid-bus-api` shall have exactly `fid-canon` and `fid-types` as its normal dependencies.
- **REQ-CORE.BUS.API-002** The bus API crate shall be mapped to layer `model` in `layers.toml`.
- **REQ-CORE.BUS.API-003** The bus API crate shall be the crate named by `bus_api` in `lint-allowlist.toml`, and listed in that file's `[core]` crates, so that it is the only CORE crate a `[surfaces]` crate may reach (REQ-CORE.BUS.LINT-004).
- **REQ-CORE.BUS.API-004** The bus API shall define `BusCall` with exactly the public fields `command` (a `fid_types` command ID), `params` (a canonical JSON value) and `dry_run` (a boolean), and with no principal, actor, on-behalf-of, identity or session field.
- **REQ-CORE.BUS.API-005** The bus API shall define `ApiReply` as exactly one of `Committed { entry, result }`, `Computed { result }`, `RequestFiled { request_id }`, `DryRun { result }` or `Failed(ApiError)`, where `entry` is a `sha256:` digest and `result` is an optional canonical JSON value (required for `DryRun`).
- **REQ-CORE.BUS.API-006** `ApiError` shall carry the bus error's code, reason and detail strings unchanged and the stage that raised it, one of `session`, `resolve`, `validate`, `authorise`, `precondition`, `route`, `handler`, `effects` or `commit`.
- **REQ-CORE.BUS.API-007** If an `ApiError` is built with a code or reason that is not 1 to 64 bytes of `[a-z0-9_]`, or with a detail longer than 2,000 characters (the `Detail` limit in the Design of `CORE.BUS.PIPE.ERR`), then the constructor shall return `invalid_api_error` and build nothing.
- **REQ-CORE.BUS.API-008** The bus API shall define `BusPort` with exactly the operations `channel()`, `principal_kind()`, `confirm_human()` and `call(&BusCall)`, none of which takes a presence provider, principal, identity, session or upgrade argument.
- **REQ-CORE.BUS.API-009** The bus API shall report principal kinds only as `fid_types::PrincipalKind`, a type with exactly the wire strings `human`, `agent`, `script`, `service`, `device` and `system` (REQ-CORE.BUS.PRIN-001) that this leaf adds to `fid-types`.
- **REQ-CORE.BUS.API-010** The bus API shall define `PortKind` with exactly the variants `cli`, `python`, `control` and `mcp`, and `ChannelInfo { kind, identity, session }` as plain read-only data that grants nothing, where `session` is the session's display label (`<label>#<number>`, CORE.BUS.PRIN) and never the session key.
- **REQ-CORE.BUS.API-011** The bus API crate shall build for the target `wasm32-unknown-unknown`.

## Acceptance criteria

- **AC-1** Given `cargo metadata --format-version 1` for the workspace, then `fid-bus-api`'s normal dependencies are exactly `fid-canon` and `fid-types`, `layers.toml` maps it to `model`, `lint-allowlist.toml` names it as `bus_api` and lists it in `[core]`, and `cargo xtask layers` and `cargo xtask lint-crate-graph` exit 0.
- **AC-2** Given a test that builds `BusCall { command, params, dry_run }` and matches every `ApiReply` variant with an exhaustive struct pattern and no `..`, then it compiles, so a field added to either type fails the build until this test and REQ-004 or REQ-005 change.
- **AC-3** (error path) Given fixture `fixtures/core/bus/api/errors.json` (hand-labelled: valid code and reason pairs from `bus-errors.json`; an empty code; a 65-byte reason; an upper-case code; a reason with `-`; details of 2,000 and 2,001 characters, the latter with a multi-byte character at the limit), when each case is built, then the valid ones build with their strings unchanged and every invalid one returns `invalid_api_error`.
- **AC-4** Given each `PrincipalKind`, when written as its wire string and parsed back, then the value is unchanged; given `Human`, `"HUMAN"`, `"user"` and `""`, then parsing fails for each.
- **AC-5** Given a test double that implements `BusPort`, when it is used as `&mut dyn BusPort`, then the trait is object-safe and the four operations are callable; `confirm_human` takes only `&mut self`.
- **AC-6** Given `cargo build -p fid-bus-api --target wasm32-unknown-unknown`, then it succeeds.

## Design

- **Registry command(s):** none.
- **Data structures:**
  - `BusCall { command: fid_types::CommandId, params: fid_canon::CValue, dry_run: bool }`.
  - `ApiReply { Committed { entry: fid_types::Digest, result: Option<CValue> }, Computed { result: Option<CValue> }, RequestFiled { request_id: String }, DryRun { result: CValue }, Failed(ApiError) }`.
  - `ApiError { code, reason, detail, stage: ApiStage }` with private fields, a checked constructor and getters. `ApiStage { Session, Resolve, Validate, Authorise, Precondition, Route, Handler, Effects, Commit }`. The eight pipeline stages are those of `fid_bus::pipeline::Stage`; `Session` covers errors raised before the pipeline runs (an unauthenticated channel, an upgrade refusal).
  - `fid_types::PrincipalKind { Human, Agent, Script, Service, Device, System }` with `as_str` and `parse`. The copies in `fid-surface`, `fid-jobs`, `fid-store` and `fid-view` move to it later (AGENTS rule 14); this leaf does not touch them.
  - `PortKind { Cli, Python, Control, Mcp }`; `ChannelInfo { kind: PortKind, identity: String, session: String }`, where `session` is the display label `<label>#<number>`; the `SessionKey` is a `fid-bus` capability and never appears here.
  - `trait BusPort { fn channel(&self) -> ChannelInfo; fn principal_kind(&self) -> PrincipalKind; fn confirm_human(&mut self) -> Result<(), ApiError>; fn call(&mut self, call: &BusCall) -> ApiReply; }`.
- **Why the crate is thin.** CONSTITUTION §2.1 puts CORE in L1, so `fid-bus-api` cannot depend on `fid-surface` (L3) for `BusRequest` or `Envelope`. It also stays below `fid-bus`, so `fid-surface` can link it and keep its wasm32 build. A fat crate that linked `fid-bus` and re-exported its types would give every surface the means to build channels and sessions; this crate gives none. A surface that implements `BusPort` itself only fools itself: it has no route to the real bus (`CORE.BUS.LINT` refuses any path that does not pass through this crate, and this crate leads nowhere).
- **What a port's answers mean.** `principal_kind` is what the bus derived for the port's channel, reported for display and for surface conveniences such as `AUTO.CLI.DISPATCH` visibility. The bus enforces policy on its own derivation at every call, never on this answer.
- **Errors:** `invalid_api_error`.
- **Determinism:** D0. Plain data and pure checks.
- **Performance budget:** not applicable.
- **Crate:** `fid-bus-api` (new, layer `model`); `fid-types` gains `PrincipalKind`.

## Test fixtures

`fixtures/core/bus/api/errors.json`: hand-labelled `ApiError` construction cases (valid pairs copied from `bus-errors.json` v1, and the invalid cases of AC-3), project-authored, CC-BY-4.0.

## Build prompt

> Create crate `crates/fid-bus-api` with normal dependencies `fid-canon` and `fid-types` only; map it to layer `model` in `layers.toml` (it is already `bus_api` and in `[core]` in `lint-allowlist.toml`). Add `PrincipalKind { Human, Agent, Script, Service, Device, System }` with wire strings `human agent script service device system`, `as_str` and a strict `parse`, to `fid-types`. In `fid-bus-api` define: `BusCall { command: CommandId, params: CValue, dry_run: bool }` (no principal, actor, on-behalf-of, identity or session field); `ApiReply { Committed { entry: Digest, result: Option<CValue> }, Computed { result: Option<CValue> }, RequestFiled { request_id: String }, DryRun { result: CValue }, Failed(ApiError) }`; `ApiError` with private `code`, `reason`, `detail` and `stage: ApiStage` (`session resolve validate authorise precondition route handler effects commit`), whose constructor returns `invalid_api_error` unless code and reason are 1..=64 bytes of `[a-z0-9_]` and detail is at most 2,000 characters; `PortKind { Cli, Python, Control, Mcp }`; `ChannelInfo { kind, identity, session }`; and the object-safe trait `BusPort { channel(&self) -> ChannelInfo; principal_kind(&self) -> PrincipalKind; confirm_human(&mut self) -> Result<(), ApiError>; call(&mut self, &BusCall) -> ApiReply }`. The crate must hold no bus, session or presence code and must build for `wasm32-unknown-unknown`. Tests: AC-1..AC-6 of spec node CORE.BUS.API, with fixture `fixtures/core/bus/api/errors.json`. Constraints: CONSTITUTION §2.1 (L1 depends on nothing above L1), §3 (principals), §8 (no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`).

## Open questions

- `BusCall` has no idempotency key, expected head, approval or R1-digest field. The in-process port never resends, and `CORE.BUS.ROUTE` refuses idempotency keys until its idempotency table is built. Each can be added later as an optional field. Adding a field is a change to REQ-004 and AC-2.
- `PrincipalKind` now has a home in `fid-types`, but four crates keep private copies. Converging them is for the nodes that own those crates.

## Children

_Leaf: none._
