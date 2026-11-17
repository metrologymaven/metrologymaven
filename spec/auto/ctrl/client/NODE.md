---
id: AUTO.CTRL.CLIENT
title: Rust control-channel client and fid ctrl diagnostics
level: 3
parent: AUTO.CTRL
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.CTRL-010]
covers: [SA.MP.02]
depends_on: [AUTO.CTRL.AUTH, AUTO.SURF.JOBS]
side_effect: R0
determinism: N
cli: [fid ctrl status, fid ctrl ping]
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.CTRL.CLIENT: Rust control-channel client and fid ctrl diagnostics

## Purpose

The client library every local consumer of the channel uses (MCP bridge mode, the CLI's app target, third-party Rust SDK clients): discovery, authentication, request multiplexing and, on disconnect, an honest per-request `not_run` / `no_reply` classification. It also provides the `fid ctrl status` and `fid ctrl ping` host subcommands, which change no state and are therefore transport diagnostics, not registry commands.

## Scope

**In:**
- Discovery-file reading and connection
- Client side of the handshake, including verifying the server proof
- Multiplexed requests with acceptance tracking through `AUTO.SURF.JOBS`
- Disconnect handling and per-request classification
- `fid ctrl status` (app found, authenticated, protocol version) and `fid ctrl ping` (round-trip time)

**Out:**
- Server side (`AUTO.CTRL.WIRE`, `AUTO.CTRL.AUTH`, `AUTO.CTRL.EXEC`)
- Clients in other languages (later; the protocol spec is the contract)

## Requirements

- **REQ-AUTO.CTRL.CLIENT-001** The client shall locate the app only through the discovery file and shall connect only to a loopback address.
- **REQ-AUTO.CTRL.CLIENT-002** If the server's proof does not verify, then the client shall close the connection and report `server_auth_failed` without sending any operation.
- **REQ-AUTO.CTRL.CLIENT-003** When the connection drops, the client shall report `not_run` for every in-flight request without an acceptance event and `no_reply` with the job ID for every accepted request without a final envelope.
- **REQ-AUTO.CTRL.CLIENT-004** `fid ctrl status` shall report whether an app was found, whether authentication succeeded and the negotiated protocol version, and shall exit 6 when no app is reachable.
- **REQ-AUTO.CTRL.CLIENT-005** `fid ctrl ping` shall report the round-trip time of an authenticated ping and execute no registry command.

## Acceptance criteria

- **AC-1** Given the test app running, when `fid ctrl status --json` runs, then it reports found, authenticated and version 1, and exits 0.
- **AC-2** Given no app, then `fid ctrl status` exits 6 and reports not found.
- **AC-3** (error path) Given a fake server that answers with a wrong server proof, then the client reports `server_auth_failed` and the fake server receives no operation after `auth`.
- **AC-4** Given three in-flight requests, one accepted, when the fake server drops the connection, then the client reports `no_reply` (with job ID) for the accepted one and `not_run` for the other two.
- **AC-5** Given a discovery file pointing at a non-loopback address, then the client refuses to connect with `ctrl_non_loopback`.

## Design

- **Registry command(s):** none; host subcommands `fid ctrl status` and `fid ctrl ping` (no state change, so not registry commands; CONSTITUTION C3 covers state changes).
- **Data structures:** `CtrlClient { conn, pending: BTreeMap<RequestId, Invocation> }`.
- **Algorithm:** read discovery → connect → `hello`/`auth` → mux; invocation tracking from `AUTO.SURF.JOBS` with this client as its `BusTransport`.
- **Errors:** `app_not_found`, `ctrl_non_loopback`, `server_auth_failed`, `version_unsupported`, transport errors.
- **Determinism:** N.
- **Performance budget:** connect plus authenticate < 20 ms on loopback [unverified until measured].
- **Crate:** `fid-ctrl`

## Test fixtures

In-process test app; scripted fake server for wrong-proof and drop cases.

## Build prompt

> In crate `fid-ctrl`, implement `CtrlClient`: read the discovery file, connect only to loopback (`ctrl_non_loopback` otherwise), run the client side of the HMAC handshake and close with `server_auth_failed` if the server proof is wrong, multiplex requests, and implement `fid_surface::BusTransport` so the invocation tracker classifies outcomes. On disconnect, report `not_run` for requests with no acceptance event and `no_reply` with job ID for accepted ones. Add host subcommands `fid ctrl status` (found, authenticated, version; exit 6 if no app) and `fid ctrl ping` (round-trip time); neither executes a registry command. Acceptance: the five cases in the spec. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; never log the token.

## Open questions

- The parent lists `fid ctrl status` and `fid ctrl ping`. They are kept as host subcommands outside the registry because they change no state; if reviewers prefer registry commands, they become R0 commands such as `ctrl.show_status@1` and gain MCP tools by the §3 rule.

## Children

_Leaf: none._
