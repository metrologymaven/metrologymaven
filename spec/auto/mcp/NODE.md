---
id: AUTO.MCP
title: MCP servers, modes and side-effect projection
level: 2
parent: AUTO
kind: subsystem
status: split
phase: P0
size: XL
owners: []
claimed_by:
satisfies: [REQ-AUTO-001, REQ-AUTO-002, REQ-AUTO-003, REQ-AUTO-005, REQ-AUTO-010]
covers: []
depends_on: [CORE, AUTO.SURF, AUTO.CTRL]
cli: [fid mcp *]
mcp: [*]
journal: []
updated: 2026-11-16
---

# AUTO.MCP: MCP servers, modes and side-effect projection

## Purpose

`fid mcp`: the Model Context Protocol servers through which agents act. It projects registry commands to MCP tools by side-effect class, keeps each server's front door small through progressive disclosure, ships generated Skills, and runs in three modes modelled on the Craft family's `mcp` subcommand: bridge (drive a running app), headless (in-process session, no window) and auto. It exists separately because the projection rules are the main agent-safety boundary of the product (C5, REQ-FID-021).

## Scope

**In:**
- Tool projection by side-effect class (CONSTITUTION §3 table)
- Server partitioning: which commands each server offers
- Front-door tool set (at most 15 per server) and progressive disclosure of the rest
- Request-only projection for W3, X1 and D1: the tool files a request for a human, never performs the action
- Modes: bridge, headless, auto; the "app closed" setting used by auto
- Transports: stdio in P0; others later
- Generated Skills (task guides) built from registry docs
- Agent principal binding per server session

**Out** (and which node owns it instead):
- Principal authentication and class enforcement at execution time (`CORE`; MCP is defence in depth, not the only gate)
- Agent branches and promotion (`CORE`)
- Proposal Inbox UI where requests land (`VIZ`)
- Control-channel protocol used by bridge mode (`AUTO.CTRL`)
- Shared envelope, timeouts and root handles (`AUTO.SURF`)

## Requirements

- **REQ-AUTO.MCP-001** The MCP tool name of every projected command shall be derived from its command ID by the CONSTITUTION §3 rule (drop `@major`, dots to underscores, `[a-z0-9_]`, at most 64 characters, `_vN` on breaking change), with no hand-written aliases.
- **REQ-AUTO.MCP-002** No server shall list, describe or invoke any M1, A0 or S1 command, in any mode.
- **REQ-AUTO.MCP-003** When an agent invokes the tool of a W3, X1 or D1 command, the server shall file a pending request for a human through the bus and shall not perform the action.
- **REQ-AUTO.MCP-004** When an agent invokes a W2 command, the commit shall land on that session's agent branch, never on the working document's main branch.
- **REQ-AUTO.MCP-005** Each server shall list at most 15 front-door tools, and every other projectable command it serves shall be discoverable and invocable through its disclosure tools.
- **REQ-AUTO.MCP-006** `fid mcp --bridge` shall drive a running app through the authenticated control channel, and shall fail with a typed error within 5 s [unverified] if no authenticated app is reachable.
- **REQ-AUTO.MCP-007** `fid mcp --headless` shall run an in-process session with no window, with the same tool list as bridge mode for the same manifest digest.
- **REQ-AUTO.MCP-008** When `fid mcp` starts with no mode flag, it shall use bridge mode if an app is running and otherwise follow the user's "app closed" setting (headless, launch the app, or refuse).
- **REQ-AUTO.MCP-009** The server shall bind the agent principal from the server session, and any principal, role or approval claim inside a tool argument shall be ignored.
- **REQ-AUTO.MCP-010** Tool descriptions and Skills shall be generated from the registry manifest, and shall contain no standards text.
- **REQ-AUTO.MCP-011** Long-running tools shall return a job handle with progress and support cancellation, using the shared `NOT_RUN` / `NO_REPLY` timeout outcomes.
- **REQ-AUTO.MCP-012** Strings returned to the agent that originate from document content shall be marked as untrusted data in the tool result (C6).

## Interfaces

- **Offers:** `fid mcp [--bridge | --headless]`; one or more MCP servers; disclosure tools; generated Skills.
- **Consumes:** `AUTO.SURF` projections and envelope; `AUTO.CTRL` client (bridge mode); CORE bus, registry and jobs (headless mode); CORE request queue for request-only tools.

## Decomposition hints

PROJ (class-based projection and request-only tools), SRV (server partitioning and front door), DISC (progressive disclosure), MODES (bridge, headless, auto), XPORT (stdio and later transports), SKILLS (generated Skills), CONF (conformance tests against a reference MCP client).

## Open questions

- Partitioning: one server per app (CONSTITUTION §2 app family, D-004) or one per domain group? Depends on D-004.
- Progressive disclosure needs a generic "describe then invoke by command ID" tool. Is a dispatcher tool a "hand-written alias" under CONSTITUTION §3? This node assumes no, because it adds no name for any one command and enforces the same class rules; confirm at review.
- The dispatcher question above is settled by D-007 / CONSTITUTION §3.1: a generated dispatcher is allowed for R0/R1 only. `AUTO.MCP.DISC` adds named tools for W1, W2 and request-only commands through a list-changed notification instead.
- Partitioning: D-004 (accepted) asks for one shared `fid` MCP surface; `AUTO.MCP.SRV` proposes one profile per app plus a core profile, all from the same binary, and leaves the declaration owner (`CORE.REG` or `ECO.APPS`) open.

## Split rationale

The split separates **what an agent may see and call** from **how a session runs**. PROJ owns the safety-critical mapping from side-effect class to tool (and the D-008 motion allowlist); SRV and DISC own which tools are listed and how the rest are reached; TAINT owns C6 on the way back. SESS owns the runtime of one session (transport, principal, jobs), and MODES owns where that session executes (bridge, headless, auto). SKILLS is a separate emitter because its input (task guides) and checks differ from tool definitions.

Alternatives considered:
- **A separate CONF (conformance) child.** Rejected: each child carries its own acceptance tests against the fake bus and a reference client; a separate conformance node would duplicate their responsibilities.
- **Transport as its own child (XPORT).** Rejected for now: only stdio is in P0 and it is inseparable from the session handshake; later transports can be added under SESS.
- **Disclosure inside SRV.** Rejected: profiles are a declaration problem; disclosure is a runtime protocol with its own class rule (§3.1, R0/R1 only for the dispatcher).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| AUTO.MCP.PROJ | Class-based tool projection and request-only tools (leaf) | 001 002 003 004 010 | none |
| AUTO.MCP.SRV | Server partitioning and front-door tool sets | 005 | none |
| AUTO.MCP.DISC | Progressive disclosure tools | 005 | none |
| AUTO.MCP.MODES | Bridge, headless and auto modes | 002 006 007 008 | none |
| AUTO.MCP.SESS | MCP session, stdio transport, principal binding and jobs (leaf) | 009 011 | none |
| AUTO.MCP.SKILLS | Generated Skills and task guides (leaf) | 010 | none |
| AUTO.MCP.TAINT | Untrusted-content marking in tool results (leaf) | 012 | none |
