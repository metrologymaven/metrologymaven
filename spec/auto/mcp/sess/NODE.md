---
id: AUTO.MCP.SESS
title: MCP session, stdio transport, principal binding and jobs
level: 3
parent: AUTO.MCP
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.MCP-009, REQ-AUTO.MCP-011]
covers: []
depends_on: [AUTO.MCP.PROJ, AUTO.SURF.JOBS, CORE.BUS]
side_effect: R0
determinism: N
cli: [fid mcp]
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.MCP.SESS: MCP session, stdio transport, principal binding and jobs

## Purpose

The MCP server runtime: the stdio transport, the session handshake, binding exactly one agent principal per session from the channel, mapping long-running tool calls onto CORE jobs with MCP progress and cancellation notifications, and returning honest `not_run` / `no_reply` outcomes. Other transports come later and reuse the same session layer.

## Scope

**In:**
- JSON-RPC over stdio framing and the MCP initialise handshake
- Session object: agent principal, profile, enabled-tool list, agent branch
- Principal binding from the channel; ignoring principal, role or approval claims in arguments
- Long-running tools: job handle in the result, progress notifications, cancellation notification → job cancel
- Bounded message sizes

**Out:**
- Tool definitions and invocation mapping (`AUTO.MCP.PROJ`)
- Modes (`AUTO.MCP.MODES`)
- Other transports (later)

## Requirements

- **REQ-AUTO.MCP.SESS-001** At session start, the server shall bind one agent principal whose identity combines the configured agent name and a fresh session UUIDv7, and shall use it for every tool call in the session.
- **REQ-AUTO.MCP.SESS-002** If a tool argument carries a field named like a principal, role, approval or branch override, then the server shall drop it before invocation and add a warning to the result.
- **REQ-AUTO.MCP.SESS-003** When a tool's command is declared long-running, the server shall return within 1 s [unverified] a result holding the job ID and outcome `accepted`, and shall send MCP progress notifications for the job when the client supplied a progress token.
- **REQ-AUTO.MCP.SESS-004** When the client sends an MCP cancellation for a request with a running job, the server shall request cancellation of that job and report its final state.
- **REQ-AUTO.MCP.SESS-005** If the server stops waiting for a bus reply, then it shall return the `not_run` or `no_reply` outcome from `AUTO.SURF.JOBS` and never a success.
- **REQ-AUTO.MCP.SESS-006** If an incoming message exceeds 4 MiB [unverified] or is not valid JSON-RPC, then the server shall answer with a JSON-RPC error and keep the session alive.

## Acceptance criteria

- **AC-1** Given fixture transcript `fixtures/mcp/handshake.jsonl` replayed over stdio, when the server runs with the fake bus, then responses equal `fixtures/mcp/handshake.expected.jsonl` except the session UUID.
- **AC-2** Given a tool call whose arguments include `{"principal": "human:alice"}`, then the fake bus receives principal kind `agent` with the session identity, no `principal` parameter, and the result carries a warning.
- **AC-3** Given the long-running fixture command `demo.fit_large@1` and a progress token, then the first result arrives in under 1 s with outcome `accepted` and a job ID, followed by at least two progress notifications.
- **AC-4** Given an MCP cancellation for that request, then `job.cancel_job@1` is sent and a later status query reports `cancelled`.
- **AC-5** (error path) Given a 5 MiB message, then the server answers with a JSON-RPC error and handles the next valid request normally.

## Design

- **Registry command(s):** none of its own; uses `job.*` from `CORE.JOB`.
- **Data structures:** `Session { id: Uuid, principal: AgentPrincipal, profile, enabled_tools: BTreeSet<ToolName>, branch: BranchRef }`.
- **Algorithm:** line-delimited JSON-RPC over stdio per the MCP specification revision pinned in the crate (2025-06-18 or later [unverified]); one async task per request; tracker from `AUTO.SURF.JOBS`.
- **Errors:** JSON-RPC parse and invalid-request errors; envelope outcomes for command errors.
- **Determinism:** N (sessions and timing).
- **Performance budget:** < 2 ms server overhead per R0 call [unverified until measured].
- **Crate:** `fid-mcp`

## Test fixtures

Recorded JSON-RPC transcripts under `fixtures/mcp/` (project-authored); fake bus and `demo.fit_large@1` fixture command; a reference MCP client from an MCP SDK used in an integration test [unverified which SDK].

## Build prompt

> In crate `fid-mcp`, implement the MCP server session over stdio (line-delimited JSON-RPC, MCP initialise handshake, pinned spec revision). Bind one agent principal per session (configured agent name plus a UUIDv7 session ID). Strip any argument field that claims a principal, role, approval or branch override, and add a warning. For long-running commands, return `accepted` with the job ID within 1 s, send progress notifications when a progress token is supplied, and map MCP cancellation to `job.cancel_job@1`. Use the `fid-surface` invocation tracker so timeouts yield `not_run` or `no_reply`, never success. Reject messages over 4 MiB or invalid JSON-RPC with a JSON-RPC error and keep serving. Acceptance: the five cases with the fake bus and recorded transcripts. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; bounded reads; the principal comes only from the session, never from arguments (C5, REQ-CORE.BUS-001).

## Open questions

- None.

## Children

_Leaf: none._
