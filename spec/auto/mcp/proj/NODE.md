---
id: AUTO.MCP.PROJ
title: Class-based tool projection and request-only tools
level: 3
parent: AUTO.MCP
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-mcp]
built_pr: 348
satisfies: [REQ-AUTO.MCP-001, REQ-AUTO.MCP-002, REQ-AUTO.MCP-003, REQ-AUTO.MCP-004, REQ-AUTO.MCP-010]
covers: []
depends_on: [AUTO.SURF.PROJ, AUTO.SURF.GEN, CORE.BUS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.MCP.PROJ: Class-based tool projection and request-only tools

## Purpose

The MCP emitter and the invocation rule behind every MCP tool. It turns each projectable command into a tool definition (name, description, input schema, result schema), never emits M1, S1 or A0 commands, marks W3, X1 and D1 tools as request-only, and invokes every tool through the bus on the MCP channel so that the bus files requests and routes W2 commits to the agent branch. It also enforces D-008 at generation time: the only motion-related tools are the hold request and read-only status.

## Scope

**In:**
- MCP `SurfaceEmitter` (surface `mcp`): `mcp-tools.json` and `surface-index.json`
- Tool descriptions generated from registry help text; request-only wording from a fixed template
- Generation-time motion allowlist check (D-008)
- Invocation adapter: tool call → bus invocation on the MCP channel

**Out:**
- Which server lists which tools (`AUTO.MCP.SRV`), disclosure (`AUTO.MCP.DISC`)
- Session, transport and jobs (`AUTO.MCP.SESS`)
- Filing requests and branch routing (`CORE.BUS`, REQ-CORE.BUS-005 and -006)
- The request inbox (`VIZ.INBOX`)

## Requirements

- **REQ-AUTO.MCP.PROJ-001** The emitter shall write one tool definition per command major whose projection is `exposed`, `named` or `request_only`, named exactly as `AUTO.SURF.PROJ` derives it.
- **REQ-AUTO.MCP.PROJ-002** The emitter shall write no tool definition, description or schema for any command whose class is M1, S1 or A0.
- **REQ-AUTO.MCP.PROJ-003** If any command that creates, approves or releases a motion run ticket would be emitted, other than the hold request and R0 status queries, then generation shall fail with `motion_tool_forbidden` naming the command.
- **REQ-AUTO.MCP.PROJ-004** The description of a request-only tool shall state that calling it files a request for a human and performs nothing.
- **REQ-AUTO.MCP.PROJ-005** When a tool is called, the adapter shall invoke its command through the bus on the MCP channel with the session's agent principal, and shall return the bus envelope (including `request_filed` with a request ID for request-only tools).
- **REQ-AUTO.MCP.PROJ-006** Tool descriptions shall be built only from the registry help text and fixed project-authored templates.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/manifest/min-classes.json`, when surfaces are generated, then `mcp-tools.json` lists tools for R0, R1, I1, W1, W2, W3, X1 and D1 commands and none for M1, S1 or A0, and equals the golden file.
- **AC-2** (error path) Given fixture `fixtures/manifest/motion-request.json` declaring `cmm.request_run@1` as a request-only command with a run-ticket effect, when generation runs, then it fails with `motion_tool_forbidden` naming it; `cmm.request_hold@1` alone passes.
- **AC-3** Given the fake bus and the W3 fixture command `demo.release_plan@1`, when its tool is called, then the bus receives channel `mcp`, principal kind `agent`, and the tool result is an envelope with outcome `request_filed` and a request ID.
- **AC-4** Given the W2 fixture command `demo.set_offset@1`, when called on the in-process bus test harness, then the commit lands on the session's agent branch and the main branch head is unchanged.
- **AC-5** Given the request-only tool definition, then its description contains the fixed request-only sentence.

## Design

- **Registry command(s):** none of its own.
- **Data structures:** `McpTool { name, title, description, input_schema, output_schema, annotations: { read_only_hint, destructive_hint, request_only }, command: CommandRef }`. Annotations: `read_only_hint` true for R0 and R1.
- **Algorithm:** filter projection by class; JSON Schema from parameter specs (quantities as `{value: string, unit: string}`); motion check by declared effects (`run_ticket.create|approve|release`) against the allowlist `{cmm.request_hold@1}` plus R0 commands.
- **Errors:** `motion_tool_forbidden`, `schema_unrepresentable`.
- **Determinism:** D0 for emitted files.
- **Performance budget:** generation for 5,000 commands < 2 s [unverified until measured].
- **Crate:** `fid-mcp`

## Test fixtures

`fixtures/manifest/min-classes.json` (with `demo.release_plan@1` W3 and `demo.set_offset@1` W2), `fixtures/manifest/motion-request.json`; golden `fixtures/mcp/min-classes.tools.json`; in-process bus harness from `CORE.BUS` tests.

## Build prompt

> In crate `fid-mcp`, implement the MCP surface emitter and tool-call adapter. From the `fid-surface` projection, emit a tool for every command whose MCP projection is `exposed`, `named` or `request_only` (name as derived, description from registry help text, JSON Schema input and output; quantities as `{value: string, unit: string}`), and nothing for M1, S1 or A0. Request-only tools get a fixed sentence saying the call files a request for a human. Fail generation with `motion_tool_forbidden` if any command declaring a run-ticket create, approve or release effect would be emitted, except `cmm.request_hold@1` and R0 status queries (D-008: agents may not file run requests over MCP). Write `mcp-tools.json` and `surface-index.json`. At run time, map each tool call to a bus invocation on channel `mcp` with the session's agent principal and return the bus envelope. Acceptance: golden tool list for `fixtures/manifest/min-classes.json`; motion check; `request_filed` for W3; W2 commit on the agent branch. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; no hand-written tool names or aliases; no standards text in descriptions.

## Open questions

- **Revise for D-036/D-037 (accepted 2026-11-17):** project `cmm.request_run@1` to agents as a named tool that the bus accepts only for H1-approved programs within envelope (D-008 as amended by D-037).
- The run-ticket effect names depend on `CMM.EXEC` declarations; the allowlist is keyed on declared effects so it follows whatever CMM declares.
- **Built in `fid-mcp`; what is left.** AC-1, AC-2, AC-3 and AC-5 are built and tested; the emitter is registered in `fid-surface-gen` and writes `mcp/mcp-tools.json` (header as its `_header` member) and `mcp/surface-index.json`. AC-4 (the W2 commit lands on the agent branch, main unchanged) needs `CORE.BUS.PIPE.STAGE` and `CORE.BUS.ROUTE`, not built: the adapter side is built (the call reaches the real authorise stage on channel `mcp` as the session's agent and is let through). The fixture commands `demo.release_plan@1` and `demo.set_offset@1` live in `fixtures/mcp/calls.json`, not in `min-classes.json`, whose bytes `AUTO.SURF.PROJ` and `AUTO.SURF.GEN` goldens pin.
- **Agreement with the bus.** Which tool is listed is the bus's own rule (`CORE.BUS.POL.PROJ` `mcp_projection`): the stricter of the declared projection and the class ceiling, and none without `mcp` in `exposure`. `fid-surface` now carries `exposure` for this. Tests compare the two for every command of the workspace registry and for every class, declared projection, `human_only` and exposure.
- **`AUTO.SURF.PROJ` refuses stricter declarations.** It requires the declared projection to equal the class table, while the bus accepts a stricter one: `request.reject_request@1` (W1, `human_only`, `never`) is refused with `projection_mismatch`, so generating surfaces from the whole workspace manifest fails (closed: nothing is listed). `AUTO.SURF.PROJ` should accept a declaration stricter than the class table.
- Tool descriptions carry no parameter descriptions: the surface projection does not keep them. Tool texts holding a control, bidi or other `REASON_FORBIDDEN_RANGES` character fail generation (`forbidden_character`); nothing is stripped.

## Children

_Leaf: none._
