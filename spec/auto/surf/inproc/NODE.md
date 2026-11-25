---
id: AUTO.SURF.INPROC
title: In-process bus transport and bus-error envelopes
level: 3
parent: AUTO.SURF
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-AUTO.SURF-005, REQ-AUTO.SURF-007]
covers: []
depends_on: [CORE.BUS.API, AUTO.SURF.JOBS, AUTO.SURF.ENV]
uses_artifacts: [CORE.BUS.PIPE.ERR]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-25
---

# AUTO.SURF.INPROC: In-process bus transport and bus-error envelopes

## Purpose

The `BusTransport` (`AUTO.SURF.JOBS`) that every surface uses when the bus runs in its own process. It turns each tracker request into one `BusPort` call (`CORE.BUS.API`), and each reply into the shared envelope (`AUTO.SURF.ENV`). It also owns the one table from bus error codes to envelope error codes, so the CLI, MCP and Python cannot drift apart when they report the same bus error. Every failed envelope carries the bus code, reason and origin in its details, under the names `AUTO.CLI.DISPATCH` already reads (`bus_code`, `bus_reason`, `bus_origin`). The transport adds nothing about who is calling. It reports the port's principal kind and channel unchanged, and it forwards `confirm_human` to the port.

## Scope

**In:**
- `InProcTransport`: a `BusTransport` over a boxed `BusPort`; `Submit`, `Cancel` and `Wait` requests; the event queue; closing
- Building the final envelope from an `ApiReply`
- The bus-code to envelope-code table and the `bus_code`, `bus_reason` and `bus_origin` details
- Passing `principal_kind`, `channel` and `confirm_human` through from the port

**Out:**
- The tracker, `BusRequest` and `BusEvent` (`AUTO.SURF.JOBS`); the envelope type and code set (`AUTO.SURF.ENV`)
- The port, sessions and upgrades (`CORE.BUS.PORT`); building ports and buses (`AUTO.SURF.COMPOSE`)
- Long-running commands handed to `CORE.JOB`, and their progress and cancellation over a job (`AUTO.SURF.JOBS`, `CORE.JOB`)
- Control-channel and host-protocol transports (`AUTO.CTRL`, `AUTO.PY`)
- Each surface's exit codes and exceptions (`AUTO.CLI.OUT`, `AUTO.MCP`, `AUTO.PY`)

## Requirements

- **REQ-AUTO.SURF.INPROC-001** When the tracker sends a `Submit` request, the in-process transport shall make exactly one `BusPort::call` with the request's command and parameters and `dry_run` false.
- **REQ-AUTO.SURF.INPROC-002** When that call returns, the transport shall queue `Accepted` and then `Finished` for the request's key, both naming one fresh UUIDv7 call ID, with the final envelope's `job_id` absent.
- **REQ-AUTO.SURF.INPROC-003** When the reply is a success, the transport shall finish with outcome `committed` (with `journal_entry_hash`), `computed`, `request_filed` (with `request_id`) or `dry_run` for `Committed`, `Computed`, `RequestFiled` and `DryRun` respectively, carrying the reply's result.
- **REQ-AUTO.SURF.INPROC-004** When the reply fails with bus code `cancelled`, the transport shall finish with outcome `cancelled`.
- **REQ-AUTO.SURF.INPROC-005** When the reply fails with any other bus code, the transport shall finish with outcome `failed`, the envelope error code that the table under Design gives for its bus code and reason, the bus detail as the message, and the details `bus_code`, `bus_reason` and `bus_origin`.
- **REQ-AUTO.SURF.INPROC-006** The transport shall set `bus_origin` to `pipeline` for stages `resolve` and `validate`, to `handler` for stage `handler`, and to `bus` for every other stage, including `session`.
- **REQ-AUTO.SURF.INPROC-007** If a failed reply's bus code is not in the table, then the transport shall finish with `internal_error` and keep the code and reason unchanged in `bus_code` and `bus_reason`.
- **REQ-AUTO.SURF.INPROC-008** When the tracker sends a `Cancel` or `Wait` request, the transport shall queue `Refused` for that request's key and attempt with reason `no_live_job`, without calling the port.
- **REQ-AUTO.SURF.INPROC-009** If the transport is closed, then `send` shall return `NotSent` and call nothing.
- **REQ-AUTO.SURF.INPROC-010** The transport shall report the port's `principal_kind` and `channel` unchanged, forward `confirm_human` to the port, and offer no operation that sets a principal kind, identity or session.

## Acceptance criteria

- **AC-1** Given a fake `BusPort` that returns `Computed { result: {"n": 1} }` and the `fid_surface` tracker over an `InProcTransport`, when one invocation of `demo.show_thing@1` with `{}` is tracked to the end, then the port received exactly one `BusCall` with that command, those parameters and `dry_run` false; the events were `Accepted` then `Finished` with the same call ID; and the final envelope is outcome `computed`, result `{"n":1}`, no `job_id`, and passes `Envelope::validate`.
- **AC-2** Given the scripted replies of `fixtures/auto/surf/inproc/replies.json` (one per `ApiReply` success form, one per row of the table under Design, one with code `cancelled`, and one with the unknown code `future_code`), when each is tracked, then each final envelope equals the hand-labelled expected envelope in the same file byte for byte (RFC 8785).
- **AC-3** Given failed replies with stages `resolve`, `validate`, `authorise`, `handler`, `commit` and `session`, then `bus_origin` is `pipeline`, `pipeline`, `bus`, `handler`, `bus` and `bus`; and given `not_found/unknown_command` from stage `handler`, then `AUTO.CLI.DISPATCH`'s absence check (`bus_origin` is `pipeline`) does not match it.
- **AC-4** (error path) Given an accepted invocation, when the tracker sends `Cancel` and then `Wait` for its key, then each yields `Refused` with reason `no_live_job` and the port's `call` count is unchanged; given a closed transport, then `send` returns `NotSent`, the port is not called and the tracker reports `not_run` (REQ-AUTO.SURF-007).
- **AC-5** Given a fake port that reports `script` and then, after `confirm_human` succeeds, `human`, when the transport is asked before and after, then it reports `script` and then `human`, and `confirm_human` reached the port exactly once.

## Design

- **Registry command(s):** none.
- **Data structures:** `InProcTransport { port: Box<dyn BusPort + Send>, queue: VecDeque<BusEvent>, keys: KeySource, closed: bool }` implementing `fid_surface::BusTransport`, with `principal_kind()`, `channel()`, `confirm_human()` and `close()`. `fn envelope_for(command: &CommandId, reply: &ApiReply) -> Envelope` is public and pure, so other transports that receive an `ApiReply` (the control-channel server) produce the same bytes.
- **Algorithm:** `send(Submit)` calls the port synchronously on the caller's thread, then queues `Accepted { key, job: call_id }` and `Finished { key, job: call_id, envelope }`. `recv` drains the queue. The call ID is a UUIDv7 from the tracker's key source. It names no `CORE.JOB` job, so it is never written to the envelope. The tracker's idempotency key is not passed to the bus: an in-process call either runs inside `send` or reports `NotSent`, so it is never resent.
- **Bus code to envelope code:**

| Bus code | Bus reason | Envelope code |
|---|---|---|
| `validation` | `unit_mismatch` | `unit_error` |
| `validation` | any other | `validation_error` |
| `policy` | `raw_path_refused` | `raw_path_refused` |
| `policy` | `outside_root` | `root_escape` |
| `policy` | `bad_handle` | `validation_error` |
| `policy` | any other | `policy_refused` |
| `precondition` | any | `precondition_failed` |
| `conflict` | any | `conflict` |
| `idempotency_mismatch` | any | `conflict` |
| `not_found` | any | `not_found` |
| `hidden_command` | any | `not_found` |
| `unauthenticated` | any | `policy_refused` |
| `upgrade_refused` | any | `policy_refused` |
| `undeclared_effect` | any | `internal_error` |
| `internal` | any | `internal_error` |
| `cancelled` | any | outcome `cancelled`, no error |

  The `policy` rows agree with `fid_surface::roots::ERROR_MAP` and `RootError::error_code` (`bad_handle` is `validation_error` there too). `hidden_command` becomes `not_found`, so a surface that does not filter (MCP for an `agent`) does not confirm that a hidden major exists (D-035); `AUTO.CLI.DISPATCH` keys on `bus_code` and is unaffected.
- **Dry run:** `BusRequest` has no mode, so every `Submit` sends `dry_run` false. When `AUTO.SURF.JOBS` adds a mode, the transport maps it onto `BusCall::dry_run`, and `ApiReply::DryRun` already maps to outcome `dry_run`.
- **Errors:** `transport_closed` (`NotSent`). Every bus failure arrives as an envelope.
- **Determinism:** N (call IDs). `envelope_for` is D0.
- **Performance budget:** < 50 µs per invocation beyond the port call [unverified until measured].
- **Crate:** `fid-surface` (module `inproc`), which gains a normal dependency on `fid-bus-api` (layer `model`). This keeps `fid-surface` free of `fid-bus` and buildable for wasm32.

## Test fixtures

`fixtures/auto/surf/inproc/replies.json`: hand-labelled `ApiReply` cases and the canonical envelope each one must produce (AC-2), project-authored, CC-BY-4.0. The fake `BusPort` and the counting port are test code in `crates/fid-surface/tests/inproc.rs`.

## Build prompt

> In crate `fid-surface`, add module `inproc` and a normal dependency on `fid-bus-api` (never `fid-bus`). Implement `InProcTransport` over `Box<dyn fid_bus_api::BusPort + Send>` as a `BusTransport`: `send(Submit)` makes exactly one `BusPort::call` with the request's command and params and `dry_run: false`, then queues `Accepted { key, job: call_id }` and `Finished { key, job: call_id, envelope }` with one fresh UUIDv7 call ID that is never written into the envelope (`job_id` absent); `send(Cancel | Wait)` queues `Refused { key, attempt, reason: "no_live_job" }` and calls nothing; after `close()`, `send` returns `NotSent`. Add the pure `envelope_for(command, &ApiReply) -> Envelope`: `Committed` -> `committed` with `journal_entry_hash`, `Computed` -> `computed`, `RequestFiled` -> `request_filed` with `request_id`, `DryRun` -> `dry_run`, bus code `cancelled` -> outcome `cancelled`, any other failure -> `failed` with the envelope code from the table in spec node AUTO.SURF.INPROC (unknown codes -> `internal_error`), the bus detail as message, and details `bus_code`, `bus_reason`, `bus_origin` (`pipeline` for stages resolve and validate, `handler` for handler, `bus` otherwise). Pass `principal_kind()`, `channel()` and `confirm_human()` through from the port; offer nothing that sets a principal. Tests: AC-1..AC-5 with `fixtures/auto/surf/inproc/replies.json`, a fake port and the existing tracker. Constraints: CONSTITUTION §3, §8 (no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`), RFC 8785 envelopes through the existing encoder, `BTreeMap` only, and the crate must still build for wasm32.

## Open questions

- The table is defined here because no other node owned the bus-to-envelope translation, and only a crate that sees `ApiError` can apply it. `AUTO.SURF.ENV` may take it over, or cite it, when it next changes. `AUTO.SURF.ENV` has no codes for `unauthenticated` or `upgrade_refused`, so both are reported as `policy_refused` and keep their bus code in the details.
- Progress and cancellation for long-running commands need the bus to hand them to `CORE.JOB` and the port to report the job. Until then every in-process call is synchronous.

## Children

_Leaf: none._
