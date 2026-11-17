---
id: AUTO.MCP.DISC
title: Progressive disclosure tools
level: 3
parent: AUTO.MCP
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-AUTO.MCP-005]
covers: []
depends_on: [AUTO.MCP.PROJ, AUTO.MCP.SESS, CORE.REG]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.MCP.DISC: Progressive disclosure tools

## Purpose

Makes every projectable command a server serves reachable even though only 15 tools are listed up front. Agents search and describe commands, call R0 and R1 commands through a generated "describe, then invoke by command ID" dispatcher (allowed for R0/R1 only, CONSTITUTION §3.1), and ask the server to add the named tools of W1, W2 and request-only commands to their session's tool list, which the server announces with the MCP list-changed notification.

## Scope

**In:**
- Search and describe over the server's commands (reusing the R0 registry query commands of `CORE.REG` where they exist)
- The dispatcher, limited to R0 and R1
- Enabling named tools in a session and the list-changed notification
- Per-session cap on enabled tools

**Out:**
- Tool definitions (`AUTO.MCP.PROJ`), profiles (`AUTO.MCP.SRV`), session state (`AUTO.MCP.SESS`)

## Requirements

- **REQ-AUTO.MCP.DISC-001** The server shall let an agent search its commands by keyword and domain and retrieve the full tool definition of any command it serves.
- **REQ-AUTO.MCP.DISC-002** The dispatcher shall invoke only commands of class R0 or R1, and if asked to invoke any other class, then it shall refuse with `policy_refused` and name the tool to enable instead.
- **REQ-AUTO.MCP.DISC-003** When an agent enables a named tool its server serves, the server shall add it to that session's tool list and send the MCP list-changed notification.
- **REQ-AUTO.MCP.DISC-004** If an agent asks to enable a tool for an M1, S1 or A0 command, or one the server does not serve, then the server shall refuse with `not_found` and reveal nothing about it.
- **REQ-AUTO.MCP.DISC-005** Disclosure tools shall be registry commands of class R0 or R1, generated like every other tool.

## Interfaces

- **Offers:** disclosure tools (names follow from their registry IDs, for example a dispatcher `<domain>.invoke_command@1`).
- **Consumes:** `CORE.REG` query commands; `AUTO.MCP.SESS` session tool list.

## Decomposition hints

SEARCH (search and describe), DISPATCH (R0/R1 dispatcher), ENABLE (session tool list and notification).

## Open questions

- The domain and IDs of the disclosure commands are not fixed: `fid mcp` is a host subcommand, so a registry domain named `mcp` would collide with it on the CLI. Candidates: reuse `registry.*` query commands from `CORE.REG` for search and describe, and a new `agent` domain for the dispatcher and enable commands. Needs agreement with `CORE.REG`.
- Client support for list-changed notifications varies [unverified]; a fallback (dispatcher-style invocation for W1/W2 through a named "invoke enabled tool" form) would conflict with §3.1 and is not proposed.

## Children

_None yet: status `open`._
