---
id: AUTO.CTRL
title: Loopback control channel
level: 2
parent: AUTO
kind: subsystem
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-AUTO-004, REQ-AUTO-005, REQ-AUTO-010]
covers: [SA.MP.02]
depends_on: [CORE, AUTO.SURF]
cli: [fid ctrl *]
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.CTRL: Loopback control channel

## Purpose

The authenticated loopback protocol through which external programs drive a running app: inspect UI state, list commands with their enabled state, execute commands, drive pointer and keyboard input, take screenshots, record and play back UI-level action traces, and list or cancel jobs. It follows the Craft family's control protocol (`research/artcraft-design-lessons.md`, items 9 to 11). MCP bridge mode, the CLI's running-app target and external SDK clients all use it.

## Scope

**In:**
- Wire protocol: framing, message schema, versioning, bounded message sizes
- Loopback-only listener and discovery of the running app
- Challenge-response authentication with keyed-MAC proofs from a user-only token file
- Operations: UI inspect, command list with enabled flag and reason, execute, input drive, screenshot, action trace record and playback, job list and cancel
- Refusal of path-bearing commands and of commands outside the client's class limits
- `fid ctrl` subcommands for status and connection testing
- Protocol spec document (offered by AUTO as an interface)

**Out** (and which node owns it instead):
- UI-side implementation of inspect, input injection and screenshot capture (`VIZ`)
- Semantic recording of commands into programs (`AUTO.REC`); action traces here are UI-level and are not programs
- Root handles and path refusal rules (`AUTO.SURF`)
- Principal authentication inside the bus (`CORE`)
- Token creation location per OS packaging (`PLAT`)

## Requirements

- **REQ-AUTO.CTRL-001** The control channel shall accept connections only on loopback addresses, and if configured with a non-loopback address, then the app shall refuse to start the listener and log a typed error.
- **REQ-AUTO.CTRL-002** The app shall create the token file readable and writable only by the owning OS user, and if the file's permissions are wider, then the app shall refuse to authenticate any client until the file is recreated.
- **REQ-AUTO.CTRL-003** Authentication shall be a challenge-response in which the client proves knowledge of the token with a keyed MAC over a fresh server nonce of at least 256 bits, and the token itself shall never be transmitted.
- **REQ-AUTO.CTRL-004** If a client sends any operation before authentication succeeds, or replays a previously used proof, then the channel shall reject it and close the connection.
- **REQ-AUTO.CTRL-005** The channel shall offer UI inspection, command listing with an enabled flag and a disabled reason, command execution, pointer and keyboard input, screenshots, action-trace record and playback, and job list and cancel.
- **REQ-AUTO.CTRL-006** Commands executed over the channel shall run under an automation principal with the same side-effect limits as an MCP agent, so M1, S1, W3 and A0 are never performed and X1 and D1 become requests.
- **REQ-AUTO.CTRL-007** If an operation carries a raw file path, then the channel shall refuse it with a typed error before execution.
- **REQ-AUTO.CTRL-008** Every message shall be bounded in size (16 MiB [unverified] for screenshots, 1 MiB [unverified] otherwise), and an oversized or malformed message shall be rejected without crashing the app.
- **REQ-AUTO.CTRL-009** Screenshots taken over the channel shall be the VIZ capture registry command, so they are journaled like any other screenshot.
- **REQ-AUTO.CTRL-010** Each execute reply shall use the shared envelope, and on disconnect the client shall be able to tell `NOT_RUN` from `NO_REPLY` for every in-flight request.
- **REQ-AUTO.CTRL-011** An action-trace playback shall reach the document only through registry commands triggered by the replayed UI input, never by writing document state directly.

## Interfaces

- **Offers:** the control-channel protocol spec (versioned); a Rust client used by `AUTO.MCP` and `AUTO.CLI`; `fid ctrl status`, `fid ctrl ping`.
- **Consumes:** CORE bus, jobs and principals; `AUTO.SURF` envelope and root rules; VIZ UI hooks (inspect, input injection, capture commands).

## Decomposition hints

WIRE (framing, schema, versioning, bounds), AUTH (token file, challenge, replay protection), OPS (operation set), TRACE (action trace record and playback), CLIENT (Rust client library), CONF (conformance and fuzz tests).

## Open questions

- Should a human be able to mark a control-channel session as human-driven (for test harnesses run by the user), lifting agent limits? This node says no; humans use the GUI or CLI.
- The human-driven session question above stays "no" here: `AUTO.CTRL.EXEC` binds every client as principal kind `agent`. How humans get working-branch commits from scripts is D-012.

## Split rationale

The split follows the protocol stack, bottom up: transport and bounds (WIRE), identity (AUTH), then the operation families split by what they touch: registry commands and jobs (EXEC), the live UI (UIOPS), and UI-level recordings (TRACE). CLIENT is the other end of the wire, kept separate because three consumers (MCP bridge, CLI app target, SDK users) depend on it and not on the server.

EXEC is separated from UIOPS because EXEC can be finished now against the bus, while UIOPS and TRACE wait on VIZ hooks (widget tree, input injection) that are not yet specified.

Alternatives considered:
- **One OPS child for all operations.** Rejected: it would mix a leaf-ready part (command execution) with parts blocked on VIZ.
- **A separate CONF/fuzz child.** Rejected: fuzzing is an acceptance criterion of WIRE, and auth vectors belong to AUTH.
- **Authentication inside WIRE.** Rejected: the security review surface of AUTH (token permissions, proofs, replay) deserves its own node and tests.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| AUTO.CTRL.WIRE | Loopback listener, discovery, framing and bounds (leaf) | 001 008 | none |
| AUTO.CTRL.AUTH | Token file and challenge-response authentication (leaf) | 002 003 004 | none |
| AUTO.CTRL.EXEC | Command listing, execution and jobs over the channel (leaf) | 005 006 007 010 | SA.MP.02 |
| AUTO.CTRL.UIOPS | UI inspection, input driving and screenshots | 005 009 | none |
| AUTO.CTRL.TRACE | UI action-trace record and playback | 005 011 | none |
| AUTO.CTRL.CLIENT | Rust control-channel client and fid ctrl diagnostics (leaf) | 010 | SA.MP.02 |
