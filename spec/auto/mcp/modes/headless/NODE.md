---
id: AUTO.MCP.MODES.HEADLESS
title: Headless in-process MCP session
level: 4
parent: AUTO.MCP.MODES
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-mcp]
satisfies: [REQ-AUTO.MCP.MODES-002, REQ-AUTO.MCP.MODES-004]
covers: []
depends_on: [AUTO.MCP.SESS, AUTO.MCP.SRV, AUTO.SURF.ROOTS, CORE.STO.LAYOUT, CORE.BUS]
side_effect: R0
determinism: N
cli: [fid mcp --headless]
mcp: []
journal: []
updated: 2026-11-17
---

# AUTO.MCP.MODES.HEADLESS: Headless in-process MCP session

## Purpose

`fid mcp --headless`: the MCP session executes commands on an in-process bus with no window, optionally over one project named by a root handle at start. It is the P0 mode (before the control channel exists) and the mode for CI and server-side agents. The tool list must equal the bridge mode's for the same manifest digest and profile, so agents cannot tell the modes apart by their tools.

## Scope

**In:**
- In-process bus transport for `AUTO.MCP.SESS`
- `--project <root handle>` at start: open with the write lock when free, read-only otherwise
- Refusing W-class tool calls with `conflict` while the project is held by another writer
- Releasing the project on session end, client disconnect and process signal
- The tool-list equality property shared with `AUTO.MCP.MODES.BRIDGE`

**Out:**
- Session protocol, principal binding and jobs (`AUTO.MCP.SESS`)
- Profiles (`AUTO.MCP.SRV`)
- Mode choice without a flag (`AUTO.MCP.MODES.SELECT`)
- Lock format and stale-lock rules (`CORE.STO.LAYOUT`)

## Requirements

- **REQ-AUTO.MCP.MODES.HEADLESS-001** When `fid mcp --headless` starts, it shall create an in-process session with no window and serve the selected profile's front-door tools over stdio.
- **REQ-AUTO.MCP.MODES.HEADLESS-002** The tool list in headless mode shall be computed only from the embedded manifest projection and the profile, so it is byte-identical to bridge mode's for the same manifest digest and profile.
- **REQ-AUTO.MCP.MODES.HEADLESS-003** The `--project` value shall be a root handle, and if it is a raw path, then `fid mcp` shall exit 3 with `raw_path_refused` before any MCP handshake.
- **REQ-AUTO.MCP.MODES.HEADLESS-004** When the project's write lock is free, the session shall take it for its lifetime; when another process holds it, the session shall open the project read-only.
- **REQ-AUTO.MCP.MODES.HEADLESS-005** While the session holds the project read-only, a call to a W1, W2 or W3 tool shall fail with `conflict` naming the lock holder and shall invoke nothing.
- **REQ-AUTO.MCP.MODES.HEADLESS-006** When the client disconnects, stdin closes or the process receives an interrupt or terminate signal, the session shall cancel its running jobs and release the project's write lock before exiting.
- **REQ-AUTO.MCP.MODES.HEADLESS-007** The tool list shall contain no tool for an M1, S1 or A0 command.

## Acceptance criteria

- **AC-1** Given fixture project `fixtures/projects/min.fid` under read-write root `projects`, when an MCP test client starts `fid mcp --headless --project root:projects/min.fid` and calls `demo_set_offset`, then the call commits to the session's agent branch, and after the client closes stdin no lock file remains.
- **AC-2** Given the same manifest and profile, when `tools/list` is called in headless mode and on the recorded bridge-mode list `fixtures/mcp/modes/bridge-list.json`, then the two lists are byte-identical.
- **AC-3** (error path) Given `--project C:\data\min.fid`, then exit 3 with `raw_path_refused` and nothing written to stdout.
- **AC-4** Given the project locked by a fake app process, when the session starts, then R0 `demo_show_offset` works and W2 `demo_set_offset` fails with `conflict` naming the holder, and the journal is unchanged.
- **AC-5** Given fixture manifest `fixtures/manifest/min-classes.json`, then `tools/list` and every page of `tools_search_tools` contain no M1, S1 or A0 command.
- **AC-6** (signal) Given a running `demo.slow_job@1`, when the process receives SIGTERM (CTRL_BREAK on Windows), then the job ends `cancelled` and the lock file is gone after exit.

## Design

- **Registry command(s):** none; host mode of `fid mcp`.
- **Data structures:** `InProcTransport` implementing the session's bus transport; `HeadlessProject { location: Locked | ReadOnly { holder } }`.
- **Algorithm:** parse flags → refuse raw path → open project (try write lock; on `locked` fall back to read-only, remembering the holder) → start session with profile → on each W-class call in read-only state, short-circuit with `conflict` → on shutdown, cancel jobs, drop the lock in a scope guard.
- **Errors:** `raw_path_refused` (exit 3), `bad_handle` (exit 2), `conflict` (tool error), storage errors.
- **Determinism:** N (process lifecycle, locks); tool-list generation is D0.
- **Performance budget:** start to first `tools/list` reply < 300 ms with `min.fid` [unverified until measured].
- **Crate:** `fid-mcp` (module `modes::headless`).

## Test fixtures

`fixtures/projects/min.fid`; `fixtures/mcp/modes/bridge-list.json` (recorded from the bridge leaf's test app, regenerated with the manifest); fake lock holder; `demo.slow_job@1` from the fake registry.

## Build prompt

> In crate `fid-mcp`, add `modes::headless` for `fid mcp --headless [--project <root handle>] [--server <profile>]`. Refuse a raw `--project` path with exit 3 `raw_path_refused` before the MCP handshake. Open the project with `ProjectLocation::open`: take the write lock if free, otherwise open read-only and remember the holder; in read-only state any W1/W2/W3 tool call fails with `conflict` naming the holder and invokes nothing. Run the `AUTO.MCP.SESS` session over stdio with an in-process bus transport; the tool list comes only from the embedded projection plus profile, so it equals bridge mode's. On stdin close, disconnect, SIGINT/SIGTERM (Windows console control events), cancel running jobs and release the lock via a scope guard. Acceptance: AC-1..AC-6. Constraints: no panic, no `unsafe` (signal handling through a vetted crate), deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; no M1/S1/A0 tool ever (CONSTITUTION §3); agents never pass raw paths.

## Open questions

- The lock rule matches `AUTO.CLI.TARGET.HEADLESS` (write lock for W-classes, read-only otherwise) but is implemented twice; a shared helper may belong in `AUTO.SURF` or `CORE.STO.LAYOUT`.

## Children

_Leaf: none._
