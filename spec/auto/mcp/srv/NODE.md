---
id: AUTO.MCP.SRV
title: Server partitioning and front-door tool sets
level: 3
parent: AUTO.MCP
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.MCP-005]
covers: []
depends_on: [AUTO.MCP.PROJ, AUTO.SURF.GEN]
uses_artifacts: [ECO.APPS]
side_effect: R0
determinism: D0
cli: [fid mcp --server *]
mcp: []
journal: []
updated: 2026-11-17
---

# AUTO.MCP.SRV: Server partitioning and front-door tool sets

## Purpose

Decides which commands each MCP server profile serves and which at most 15 of them are front-door tools listed at connection time. Following D-004 (one shared `fid` MCP surface over a family of apps), there is one `core` profile plus one profile per app (Inspect, Probe, Network, Report), all served by the same `fid mcp` binary and selected with `--server <profile>`. This node owns the profile file format, the generator that resolves profiles against the manifest, and the CI check.

## Scope

**In:**
- Profile file format `fid.mcp-profile` version 1 and its JSON Schema
- The `core` profile file (authored here); app profile files are read as artifacts published by `ECO.APPS`
- Resolution of a profile against the projection: served commands and the front-door list
- Generated `mcp-profiles.json` index, stamped with the manifest digest
- CI check: front-door size, unserved projectable commands, invalid entries
- `--server <profile>` selection and its usage error

**Out:**
- Disclosure mechanics and the disclosure commands themselves (`AUTO.MCP.DISC`)
- Tool definitions and class rules (`AUTO.MCP.PROJ`)
- The content of each app's profile (`ECO.APPS`)
- Session runtime (`AUTO.MCP.SESS`)

## Requirements

- **REQ-AUTO.MCP.SRV-001** A profile file shall declare a profile name (`[a-z][a-z0-9_]{1,31}`), the domains it serves, optional extra command IDs, and an ordered front-door list of command IDs, and nothing else.
- **REQ-AUTO.MCP.SRV-002** The generator shall resolve each profile's served set as every projectable command (projection not `none`) in its domains plus its extra command IDs, and shall drop nothing silently.
- **REQ-AUTO.MCP.SRV-003** Every profile's front door shall contain every command of the disclosure domain `tools` and shall hold at most 15 tools in total.
- **REQ-AUTO.MCP.SRV-004** If a profile exceeds 15 front-door tools, names a front-door command it does not serve, or names a command that is unknown or has projection `none`, then generation shall fail with `profile_invalid` naming the profile and the entry.
- **REQ-AUTO.MCP.SRV-005** If any projectable command in the manifest is served by no profile, then the CI check shall fail with `command_unserved` naming the command.
- **REQ-AUTO.MCP.SRV-006** The generator shall write `mcp-profiles.json` as RFC 8785 canonical JSON, ordered by profile name and command ID, with the manifest digest.
- **REQ-AUTO.MCP.SRV-007** When `fid mcp --server <profile>` names an unknown profile, `fid mcp` shall exit 2 with `unknown_profile` listing the known profiles, before any MCP handshake.
- **REQ-AUTO.MCP.SRV-008** When `fid mcp` starts without `--server`, it shall use the `core` profile.

## Acceptance criteria

- **AC-1** Given fixture manifest `fixtures/manifest/min-classes.json` and profiles `fixtures/mcp/profiles/core.toml` and `fixtures/mcp/profiles/inspect.toml`, when the generator runs, then `mcp-profiles.json` equals golden `fixtures/mcp/profiles/expected.json` byte for byte, with no M1, S1 or A0 command in any served set.
- **AC-2** (error path) Given `fixtures/mcp/profiles/too-many.toml` with 16 front-door entries, then generation fails with `profile_invalid` naming the profile and the sixteenth entry.
- **AC-3** (error path) Given a profile whose front door omits `tools.search_tools@1`, then generation fails with `profile_invalid`.
- **AC-4** (error path) Given a profile front door naming `demo.move_axis@1` (M1), then generation fails with `profile_invalid` naming it.
- **AC-5** (error path) Given a manifest with projectable `demo.orphan@1` in a domain no profile serves, then the CI check fails with `command_unserved` naming it.
- **AC-6** Given `fid mcp --server nosuch`, then exit 2 with `unknown_profile` listing `core` and `inspect`, and nothing is written to stdout as MCP traffic.
- **AC-7** (determinism) Given the same inputs on Windows and Linux, the generated `mcp-profiles.json` is byte-identical.

## Design

- **Registry command(s):** none; build-time generator plus a host flag on `fid mcp`.
- **Data structures:** profile file (TOML): `format = "fid.mcp-profile"`, `version = 1`, `name`, `title`, `domains = [..]`, `commands = [..]`, `front_door = [..]`; `ResolvedProfile { name, served: BTreeSet<CommandRef>, front_door: Vec<CommandRef> }`; `ProfileIndex { manifest_digest, profiles: Vec<ResolvedProfile> }`.
- **Algorithm:** parse each profile file with a bounded reader (max 64 KiB); validate against `schemas/mcp-profile-1.json`; resolve against the projection from `AUTO.SURF.PROJ` via `AUTO.SURF.GEN`; check rules in REQ-003..005; emit the index. At runtime `fid mcp` loads the embedded index and picks the profile.
- **Errors:** `profile_parse`, `profile_invalid`, `command_unserved`, `unknown_profile`.
- **Determinism:** D0; pure function of manifest bytes and profile files.
- **Performance budget:** resolving 10 profiles over 5,000 commands in < 100 ms [unverified until measured].
- **Crate:** `fid-mcp` (module `profiles`), invoked from the `cargo xtask gen-surfaces` step.

## Test fixtures

Project-authored profile files under `fixtures/mcp/profiles/` (`core.toml`, `inspect.toml`, `too-many.toml`, expected golden), CC-BY-4.0; `fixtures/manifest/min-classes.json` extended with a `tools` domain of four commands.

## Build prompt

> In crate `fid-mcp`, add module `profiles`. Define the TOML profile format `fid.mcp-profile` v1 (`name`, `title`, `domains`, `commands`, `front_door`) and publish its JSON Schema as `schemas/mcp-profile-1.json`. In the `cargo xtask gen-surfaces` step, read the `core` profile from this crate and each app profile from the paths `ECO.APPS` publishes, resolve each against the surface projection: served = projectable commands (projection not `none`) in the listed domains plus extra IDs; front door = the ordered list, which must include every `tools.*` command and be at most 15 long, and every entry must be served and projectable (`profile_invalid` otherwise). Fail CI with `command_unserved` if any projectable command is served by no profile. Write `mcp-profiles.json` (RFC 8785, sorted, with manifest digest) and embed it in `fid mcp`. `fid mcp --server <name>` selects a profile, default `core`; an unknown name exits 2 with `unknown_profile` before any MCP handshake. Acceptance: AC-1..AC-7 with the fixtures under `fixtures/mcp/profiles/`. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; bounded parsing; `BTreeMap`/`BTreeSet` only for ordering; M1, S1 and A0 can never be served (CONSTITUTION §3).

## Open questions

- The declaration owner question is resolved here without a registry field: the format belongs to this node, `ECO.APPS` writes one profile file per app (artifact dependency; `ECO.APPS` already depends on AUTO, so a code dependency would be a cycle). `ECO.APPS` must name its profile files in its Interfaces.
- The disclosure domain name `tools` is chosen in `AUTO.MCP.DISC`; if `CORE.REG` reserves domain names differently, only the constant changes.

## Children

_Leaf: none._
