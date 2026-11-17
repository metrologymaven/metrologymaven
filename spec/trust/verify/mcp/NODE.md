---
id: TRUST.VERIFY.MCP
title: fid-verify local stdio MCP server
level: 3
parent: TRUST.VERIFY
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-TRUST.VERIFY-006]
covers: []
depends_on: [TRUST.VERIFY.CLI]
uses_artifacts: [AUTO.MCP.PROJ, AUTO.MCP.TAINT, AUTO.MCP.SESS]
side_effect: R1
determinism: D0
cli: [fid-verify mcp]
mcp: [verify_package, verify_check_signature, verify_check_chain, verify_replay, verify_inspect_package]
journal: [verify.package@1]
updated: 2026-11-17
---

# TRUST.VERIFY.MCP: fid-verify local stdio MCP server

## Purpose

Lets an agent verify a Result Package without the full app: `fid-verify mcp --root <dir>...` starts a local stdio MCP server exposing only the read and compute verification tools. Packages are addressed by handles inside the roots the user passed on the command line; the agent cannot pass paths or widen the roots. Tool definitions are generated from the same registry entries as the CLI.

## Scope

**In:**
- stdio transport, server start-up with `--root` arguments
- Tool list: `verify_package`, `verify_check_signature`, `verify_check_chain`, `verify_replay`, `verify_inspect_package`
- Handle resolution: `root_id` plus relative path, normalised and confined to the root
- A listing tool for files in the roots (R0) generated from a `verify.list_files@1` entry
- Package content returned to the agent typed as untrusted data (C6)

**Out:**
- The checks and report (`TRUST.VERIFY.CLI`, `TRUST.VERIFY.REPORT`)
- The generic MCP generator and transport library (`AUTO.MCP`)
- The main `fid mcp` server (`AUTO.MCP`)

## Requirements

- **REQ-TRUST.VERIFY.MCP-001** The server shall expose only tools whose registry class is R0 or R1, and a CI test shall fail if any other class appears in its tool list.
- **REQ-TRUST.VERIFY.MCP-002** The server shall resolve every package argument as a handle within a root given on its command line, and shall reject absolute paths, `..` segments, symlinks leaving the root and unknown root IDs with `verify.bad_handle`.
- **REQ-TRUST.VERIFY.MCP-003** The server shall offer no tool that adds, removes or changes roots.
- **REQ-TRUST.VERIFY.MCP-004** Every string taken from package content and returned in a tool result shall be marked as untrusted content per the AUTO.MCP convention.
- **REQ-TRUST.VERIFY.MCP-005** For the same inputs, each tool's structured result shall equal the corresponding CLI `--json` output.
- **REQ-TRUST.VERIFY.MCP-006** The server shall make no network connection, and a CI test shall run it with networking disabled.

## Acceptance criteria

- **AC-1** Given `fid-verify mcp --root fixtures/pkg/golden` and an MCP test client, when `tools/list` is called, then exactly the five verification tools plus `verify_list_files` are listed.
- **AC-2** Given `verify_package` with handle `{root: 0, path: "points.zip"}` and replay enabled, then the result equals `fid-verify points.zip --replay --json` from `TRUST.VERIFY.CLI` AC-1.
- **AC-3** (error path) Given handles with `../x`, `/etc/passwd`, `C:\\x` and root 7, then each fails with `verify.bad_handle` and no file outside the root is opened (checked by an open-file audit in the test harness).
- **AC-4** (error path) Given a package whose ResultRecord characteristic name contains an instruction-like string, then the returned string is wrapped as untrusted content.
- **AC-5** Given the server run in a network-less container, then AC-2 passes.

## Design

- **Registry command(s):** reuses `verify.*@1` entries; adds `verify.list_files@1` (R0, lists files in roots). MCP names generated per CONSTITUTION §3. The standalone server writes no journal; when the same tools run inside `fid mcp` they go to the Activity Log.
- **Data structures:** `RootTable(Vec<CanonicalPath>)`, `Handle { root: u16, path: RelPath }`.
- **Algorithm:** canonicalise roots at start-up; resolve handle by joining and canonicalising, then prefix-check against the root; open read-only.
- **Errors:** `verify.bad_handle`, plus each tool's errors.
- **Determinism:** D0 for results.
- **Performance budget:** tool call overhead under 20 ms beyond the check itself [unverified until measured].
- **Crate:** `fid-verify` (module `mcp`). It implements a minimal stdio MCP server itself and follows the published `AUTO.MCP` conventions (tool naming and projection from `AUTO.MCP.PROJ`, untrusted-content marking from `AUTO.MCP.TAINT`, stdio framing from `AUTO.MCP.SESS`) as artifacts, not as a crate dependency: `AUTO` is layer L3, and `TRUST.VERIFY.REPRO`'s crate allowlist excludes it.

## Test fixtures

- Golden packages and a scripted MCP test client (JSON-RPC transcripts in `fixtures/verify/mcp/`).

## Build prompt

> In crate `fid-verify`, add module `mcp` and the `fid-verify mcp --root <dir>...` subcommand: a minimal local stdio MCP server (implemented in this crate, following the published AUTO.MCP naming, projection, untrusted-marking and stdio conventions; do not link any AUTO or CORE crate) with tools generated from the registry entries `verify.package@1`, `verify.check_signature@1`, `verify.check_chain@1`, `verify.replay@1`, `verify.inspect_package@1` and a new `verify.list_files@1` (R0). Only R0/R1 tools may be listed (CI check). Package arguments are handles `{root, path}` confined to the command-line roots (reject absolute, `..`, escaping symlinks, unknown roots with `verify.bad_handle`); no tool changes roots; strings from package content are marked untrusted. Results must equal the CLI JSON. No network. Tests: the five acceptance criteria with transcripts in `fixtures/verify/mcp/`. Constraints: CONSTITUTION §3 (handles only for agents), C6, C7, C8, §8.

## Open questions

- The handle shape must match AUTO.SURF's root-handle model once that is fixed; adapt the field names, not the rules.

## Children

_Leaf: none._
