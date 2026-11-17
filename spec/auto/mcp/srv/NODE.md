---
id: AUTO.MCP.SRV
title: Server partitioning and front-door tool sets
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
depends_on: [AUTO.MCP.PROJ]
cli: [fid mcp --server *]
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.MCP.SRV: Server partitioning and front-door tool sets

## Purpose

Decides which commands each MCP server offers and which at most 15 of them are front-door tools listed at connection time. Following D-004 (one shared `fid` MCP surface over a family of apps), the starting proposal is one server profile per app (Inspect, Probe, Network, Report) plus a core profile, all served by the same `fid mcp` binary.

## Scope

**In:**
- Server profiles: which domains and commands each profile serves
- Front-door selection per profile (at most 15 tools, including disclosure tools)
- Profile declaration format (in the registry or in a generated profile file) and its CI check
- `fid mcp --server <profile>` selection

**Out:**
- Disclosure mechanics for non-front-door commands (`AUTO.MCP.DISC`)
- Tool definitions (`AUTO.MCP.PROJ`)

## Requirements

- **REQ-AUTO.MCP.SRV-001** Each server profile shall list at most 15 front-door tools, counting its disclosure tools.
- **REQ-AUTO.MCP.SRV-002** Every projectable command shall be served by at least one profile.
- **REQ-AUTO.MCP.SRV-003** Profiles and front-door sets shall be generated from declarations, and a CI check shall fail when a profile exceeds 15 front-door tools or leaves a projectable command unserved.
- **REQ-AUTO.MCP.SRV-004** When `fid mcp --server <profile>` names an unknown profile, the server shall exit with a usage error before any MCP handshake.

## Interfaces

- **Offers:** server profiles; `fid mcp --server <profile>`.
- **Consumes:** `AUTO.MCP.PROJ` tool definitions; app definitions from `ECO.APPS`.

## Decomposition hints

PROFILE (declaration format and generator), FRONT (front-door selection rules and CI check). Likely one M leaf once the declaration owner is settled.

## Open questions

- Where front-door choices are declared: per command in the registry (a `front_door: [profile]` field owned by `CORE.REG`) or in a per-app profile file owned by `ECO.APPS`. Both are generated inputs; the choice belongs to whoever expands `CORE.REG` and `ECO.APPS`.

## Children

_None yet: status `open`._
