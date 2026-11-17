---
id: AUTO.MCP.DISC
title: Progressive disclosure tools
level: 3
parent: AUTO.MCP
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.MCP-005]
covers: []
depends_on: [AUTO.MCP.PROJ, AUTO.MCP.SESS, AUTO.MCP.SRV, CORE.REG]
side_effect: [R0, R0, R1, R0]
determinism: D0
cli: [fid tools search-tools, fid tools describe-tool, fid tools invoke-readonly, fid tools enable-tools]
mcp: [tools_search_tools, tools_describe_tool, tools_invoke_readonly, tools_enable_tools]
journal: [tools.search_tools@1, tools.describe_tool@1, tools.invoke_readonly@1, tools.enable_tools@1]
updated: 2026-11-17
---

# AUTO.MCP.DISC: Progressive disclosure tools

## Purpose

Makes every projectable command a server profile serves reachable even though only 15 tools are listed up front. Four generated registry commands in the `tools` domain let an agent search and describe the profile's commands, call R0 and R1 commands through a "describe, then invoke by command ID" dispatcher (allowed for R0/R1 only, CONSTITUTION §3.1), and enable the named tools of W1, W2 and request-only commands in its session, which the server announces with the MCP list-changed notification. These four commands are always on every profile's front door (`AUTO.MCP.SRV`).

## Scope

**In:**
- `tools.search_tools@1`, `tools.describe_tool@1`, `tools.invoke_readonly@1`, `tools.enable_tools@1`: definitions, handlers and their filtering by profile and class
- The session's enabled-tool set and its cap
- Sending `notifications/tools/list_changed` after enabling
- Behaviour of the four commands outside an MCP session (CLI, Python)

**Out:**
- Tool definitions and the call adapter (`AUTO.MCP.PROJ`)
- Profiles and front-door lists (`AUTO.MCP.SRV`)
- Session state, transport and principal (`AUTO.MCP.SESS`)
- Untrusted marking of returned strings (`AUTO.MCP.TAINT`)

## Requirements

- **REQ-AUTO.MCP.DISC-001** When `tools.search_tools@1` runs with a keyword, a domain or both, it shall return the matching commands served by the caller's profile, each with tool name, title, side-effect class and whether it is enabled in the session, ordered by command ID, at most 50 per page with a continuation cursor.
- **REQ-AUTO.MCP.DISC-002** When `tools.describe_tool@1` names a command the profile serves, it shall return that command's full MCP tool definition as `AUTO.MCP.PROJ` emits it.
- **REQ-AUTO.MCP.DISC-003** If `tools.describe_tool@1` or `tools.enable_tools@1` names a command of class M1, S1 or A0, an unknown command or one the profile does not serve, then it shall fail with `not_found` and the same message text for all three cases.
- **REQ-AUTO.MCP.DISC-004** `tools.search_tools@1` shall never return a command of class M1, S1 or A0 or one the profile does not serve.
- **REQ-AUTO.MCP.DISC-005** When `tools.invoke_readonly@1` names a served command of class R0 or R1, it shall invoke that command through the same adapter, principal and argument checks as its named tool and return that command's envelope.
- **REQ-AUTO.MCP.DISC-006** If `tools.invoke_readonly@1` names a served command of any class other than R0 or R1, then it shall fail with `policy_refused`, invoke nothing, and name the tool to enable instead.
- **REQ-AUTO.MCP.DISC-007** When `tools.enable_tools@1` names served commands, the server shall add their tools to the session's enabled set and send one `notifications/tools/list_changed`, and the next `tools/list` shall contain them.
- **REQ-AUTO.MCP.DISC-008** If enabling would take the session above 64 [unverified] enabled tools beyond the front door, then `tools.enable_tools@1` shall fail with `tool_cap_reached`, enable none of the requested tools, and name the cap.
- **REQ-AUTO.MCP.DISC-009** When `tools.enable_tools@1` is invoked outside an MCP session (CLI or Python), it shall fail with `no_mcp_session` and change nothing; the other three commands shall work there against the profile given by `--server` or the `core` profile.

## Acceptance criteria

- **AC-1** Given fixture manifest `fixtures/manifest/min-classes.json` and profile `fixtures/mcp/profiles/core.toml` on the fake bus, when an MCP test client calls `tools_search_tools { keyword: "offset" }`, then it receives `demo.set_offset@1` and `demo.show_offset@1` with classes W2 and R0, and nothing of class M1, S1 or A0.
- **AC-2** (error path) Given `tools_describe_tool` and `tools_enable_tools` for `demo.move_axis@1` (M1), for `demo.unknown@1` and for a command served only by another profile, then all six calls fail with `not_found` and byte-identical messages.
- **AC-3** Given `tools_invoke_readonly { command: "demo.show_offset@1", args: {..} }`, then the fake bus receives `demo.show_offset@1` with the session's agent principal and the result equals calling the named tool directly.
- **AC-4** (error path) Given `tools_invoke_readonly` for `demo.set_offset@1` (W2), then it fails with `policy_refused` naming `demo_set_offset`, and the fake bus receives nothing.
- **AC-5** Given `tools_enable_tools { commands: ["demo.set_offset@1"] }`, then the client receives one `notifications/tools/list_changed` and the next `tools/list` contains `demo_set_offset`.
- **AC-6** (error path) Given a session with 64 enabled tools, when one more is requested, then `tool_cap_reached`, the enabled set is unchanged and no notification is sent.
- **AC-7** Given `fid tools enable-tools --commands demo.set_offset@1 --json` on the CLI, then outcome `failed` with `no_mcp_session` and exit 4; `fid tools search-tools --keyword offset --json` succeeds.
- **AC-8** (determinism) Given the same manifest, profile and query, two searches return byte-identical results.

## Design

- **Registry command(s):**
  - `tools.search_tools@1` (R0): `keyword: Option<Untrusted<String>>`, `domain: Option<DomainName>`, `cursor: Option<Cursor>` → `{ items: Vec<ToolSummary>, next: Option<Cursor> }`.
  - `tools.describe_tool@1` (R0): `command: CommandRef` → `McpTool`.
  - `tools.invoke_readonly@1` (R1): `command: CommandRef`, `args: Json` → the invoked command's envelope.
  - `tools.enable_tools@1` (R0; changes only ephemeral session state, never document, record or configuration state): `commands: Vec<CommandRef>` (1..16) → `{ enabled: Vec<ToolName> }`.
  - Reads (R0/R1) go to the Activity Log only, per D-031.
- **Data structures:** `ToolSummary { tool, command, title, side_effect, enabled }`; the session's `enabled_tools: BTreeSet<ToolName>` lives in `AUTO.MCP.SESS`'s `Session`.
- **Algorithm:** every handler first filters the projection by `served(profile)` and `projection != none`; search matches keyword case-insensitively against command ID, title and help summary with a fixed ASCII case fold; failures for hidden, unknown and unserved commands share one code path to keep messages identical.
- **Errors:** `not_found`, `policy_refused`, `tool_cap_reached`, `no_mcp_session`, `bad_cursor`.
- **Determinism:** D0 for search and describe; dispatcher results inherit the invoked command's class.
- **Performance budget:** search over 5,000 commands < 20 ms [unverified until measured].
- **Crate:** `fid-mcp` (module `disclosure`); commands registered from this crate.

## Test fixtures

`fixtures/manifest/min-classes.json` extended with `demo.show_offset@1` (R0) and the four `tools.*` commands; `fixtures/mcp/profiles/core.toml` and `inspect.toml` from `AUTO.MCP.SRV`; fake bus; MCP test client from `AUTO.MCP.SESS` tests.

## Build prompt

> In crate `fid-mcp`, add module `disclosure` and register four commands in domain `tools`: `tools.search_tools@1` (R0; keyword/domain search over the caller profile's served, projectable commands; 50 per page with cursor; returns tool, command, title, class, enabled flag), `tools.describe_tool@1` (R0; returns the `McpTool` that `AUTO.MCP.PROJ` emits), `tools.invoke_readonly@1` (R1; dispatches only R0/R1 served commands through the same adapter, principal and raw-path gate as the named tool; any other class → `policy_refused` naming the tool to enable, nothing invoked), `tools.enable_tools@1` (R0; adds up to 16 served tools per call to the session's enabled set, cap 64 beyond the front door → `tool_cap_reached` and nothing enabled; sends one `notifications/tools/list_changed`). M1, S1, A0, unknown and unserved commands all fail with the same `not_found` message and never appear in search. Outside an MCP session `enable_tools` fails with `no_mcp_session`; the others use the `--server` profile or `core`. CLI and Python surfaces are generated as for every command. Acceptance: AC-1..AC-8 with the fixtures above. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; the dispatcher is R0/R1 only (CONSTITUTION §3.1); keyword input is untrusted text (C6); reads are journaled only to the Activity Log (D-031).

## Open questions

- Domain name `tools` is chosen here (the `mcp` name would collide with the `fid mcp` host subcommand, and `agent` is the AGENT domain's namespace). `CORE.REG` should confirm no other node claims `tools`.
- Cross-domain: `registry.list_commands@1` and `registry.describe_command@1` (`CORE.REG.QRY`, R0) are themselves exposed on MCP and return M1, S1 and A0 commands, which conflicts with REQ-AUTO.MCP-002 ("no server shall list or describe" them). Either `CORE.REG.QRY` filters by principal kind or `AUTO.MCP.SRV` profiles must exclude the `registry` domain; this node does not reuse them.
- Client support for list-changed notifications varies [unverified]; for clients without it, the agent must reconnect to see enabled tools. A W1/W2 dispatcher fallback is not offered (CONSTITUTION §3.1).

## Children

_Leaf: none._
