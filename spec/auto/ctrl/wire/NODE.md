---
id: AUTO.CTRL.WIRE
title: Loopback listener, discovery, framing and bounds
level: 3
parent: AUTO.CTRL
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-ctrl]
satisfies: [REQ-AUTO.CTRL-001, REQ-AUTO.CTRL-008]
covers: []
depends_on: [AUTO.SURF.ENV]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.CTRL.WIRE: Loopback listener, discovery, framing and bounds

## Purpose

The transport layer of the control channel: a listener bound only to loopback, a discovery file that lets local clients find a running app without any secret in it, a versioned JSON-lines message schema, and hard size bounds so that no malformed or oversized message can crash or stall the app.

## Scope

**In:**
- Listener on `127.0.0.1` or `::1`, ephemeral port; refusal of any other address
- Discovery file (port, process ID, app instance ID, protocol versions) in a per-user runtime directory
- Message schema: `{v, id, op, params}` requests, `{v, id, envelope | event}` replies, version negotiation in `hello`
- JSON-lines framing with per-operation size limits; fuzz targets

**Out:**
- Authentication (`AUTO.CTRL.AUTH`), operations (`AUTO.CTRL.EXEC`, `AUTO.CTRL.UIOPS`, `AUTO.CTRL.TRACE`)
- Runtime-directory location per OS (`PLAT`)

## Requirements

- **REQ-AUTO.CTRL.WIRE-001** The listener shall bind only to `127.0.0.1` or `::1`, and if configured with any other address, then the app shall not start the listener and shall log `ctrl_non_loopback`.
- **REQ-AUTO.CTRL.WIRE-002** The app shall write a discovery file holding port, process ID, app instance ID and supported protocol versions, with no token or secret, readable only by the owning user, and shall delete it on clean exit.
- **REQ-AUTO.CTRL.WIRE-003** Every message shall be one line of UTF-8 JSON; a line longer than 1 MiB [unverified], or 16 MiB [unverified] for a screenshot reply, shall be rejected with `message_too_large` and the connection closed.
- **REQ-AUTO.CTRL.WIRE-004** If a line is not valid JSON or does not match the schema, then the server shall reply `malformed_message` with the request ID when one can be read, and the app shall keep running.
- **REQ-AUTO.CTRL.WIRE-005** When a client's `hello` lists no protocol version the server supports, the server shall reply `version_unsupported` with its supported versions and close the connection.
- **REQ-AUTO.CTRL.WIRE-006** The server shall accept at most 8 [unverified] concurrent connections and refuse further ones with `too_many_connections`.

## Acceptance criteria

- **AC-1** Given config `ctrl.address = "0.0.0.0"`, when the app starts in the test harness, then no socket is bound and the log contains `ctrl_non_loopback`.
- **AC-2** Given a running test app, then the discovery file exists with the bound port, contains no field named like token or secret, and has owner-only permissions; after clean shutdown it is gone.
- **AC-3** (error path) Given a 2 MiB line, then the reply is `message_too_large` and the connection closes; a new connection still works.
- **AC-4** (fuzz) Given the `cargo fuzz` target `ctrl_frame` run for 10 minutes in CI on the corpus `fixtures/ctrl/frames/`, then there is no crash, hang over 1 s per input or memory growth beyond 64 MiB.
- **AC-5** (error path) Given `hello` with versions `[99]`, then the reply is `version_unsupported` listing `[1]`.

## Design

- **Registry command(s):** none.
- **Data structures:** `Request { v: u16, id: u64, op: OpName, params: CanonicalJson }`; `Reply { v, id, body: Envelope | Event }`; `Discovery { port, pid, instance_id, versions }`.
- **Algorithm:** bounded line reader (reads at most limit + 1 bytes before deciding); schema validation per op; one task per connection.
- **Errors:** `ctrl_non_loopback`, `message_too_large`, `malformed_message`, `version_unsupported`, `too_many_connections`.
- **Determinism:** N (network timing); framing functions are pure.
- **Performance budget:** < 1 ms round trip for a 1 KiB ping on loopback [unverified until measured].
- **Crate:** `fid-ctrl`

## Test fixtures

Seed corpus `fixtures/ctrl/frames/` (valid and malformed lines, project-authored); in-process test app harness.

## Build prompt

> In crate `fid-ctrl`, implement the control-channel transport. Bind only to 127.0.0.1 or ::1 on an ephemeral port; any other configured address → do not listen, log `ctrl_non_loopback`. Write a discovery file (port, pid, instance ID, protocol versions; no secrets) with owner-only permissions, removed on clean exit. Messages are JSON lines `{v, id, op, params}` with replies `{v, id, envelope|event}`; read with a bounded reader (1 MiB, 16 MiB for screenshot replies), reject oversize with `message_too_large` and close, reject invalid JSON or schema with `malformed_message` and keep running. Version negotiation in `hello` (`version_unsupported`). At most 8 connections. Add a `cargo fuzz` target `ctrl_frame`. Acceptance: the five cases in the spec. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; all input hostile, parsing bounded and fail-closed (CONSTITUTION §8).

## Open questions

- Unix-domain sockets and Windows named pipes would avoid TCP entirely; the parent requires loopback, and the Craft protocol uses TCP loopback. Revisit if a security review prefers OS-local IPC.

## Children

_Leaf: none._
