---
id: ECO.SDK.ISO
title: Plugin isolation, capability grants and fault containment
level: 3
parent: ECO.SDK
kind: module
status: open
phase: P1b
size: XL
owners: []
claimed_by:
satisfies: [REQ-ECO.SDK-004, REQ-ECO.SDK-005]
covers: []
depends_on: [ECO.SDK.EXT, ECO.SDK.PKG, CORE.BUS, CORE.JOB]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.SDK.ISO: Plugin isolation, capability grants and fault containment

## Purpose

Runs plugin code so that it can reach only what its manifest declares and the user has granted (file roots, network, devices, other plugins), and so that a plugin that faults, hangs or returns malformed data is stopped without taking the session down. It owns the isolation mechanism, the capability model and its grant commands, and the denial and fault reporting.

## Scope

**In:**
- Choice of isolation mechanism per plugin tier (for example WASM components, out-of-process host, or both), with a threat model
- Capability model: file-root handles (never raw paths), network, device classes, inter-plugin calls
- Grant and revoke commands (A0) and their persistence
- Time budgets per call, cancellation, fault capture, malformed-result rejection
- Numeric profile of plugin values of record (KNP-1) inside the chosen mechanism

**Out** (and which node owns it instead):
- Which extension points exist (owned by `ECO.SDK.EXT`)
- Driver host process model for instruments (owned by `HAL.HOST`, which may reuse this mechanism)
- Job and cancellation framework (owned by `CORE.JOB`)

## Requirements

- **REQ-ECO.SDK.ISO-001** If plugin code requests a capability not both declared in its manifest and granted by the user, then the host shall deny the request, return a typed denial to the plugin and journal the denial with plugin ID and capability.
- **REQ-ECO.SDK.ISO-002** Plugins shall receive file access only as handles to approved roots, and shall never receive or be able to construct raw file-system paths outside them.
- **REQ-ECO.SDK.ISO-003** If a plugin call exceeds its declared time budget, then the host shall stop the call within 1 s of the budget [unverified], keep the session running and report the plugin identity and the budget exceeded.
- **REQ-ECO.SDK.ISO-004** If plugin code faults or returns data that fails schema validation, then the host shall discard the result, keep the session running and report the plugin identity and fault.
- **REQ-ECO.SDK.ISO-005** Granting or revoking a plugin capability shall be an A0 registry command.
- **REQ-ECO.SDK.ISO-006** Values of record computed by plugin code shall satisfy the KNP-1 numeric profile and the declared determinism class, verified by a replay test per plugin algorithm.

## Interfaces

- **Offers:** plugin runtime; capability model; `plugin.grant_capability@1` and `plugin.revoke_capability@1` (A0); denial and fault events.
- **Consumes:** ECO.SDK.EXT registrations; ECO.SDK.PKG manifest capabilities; CORE.BUS; CORE.JOB budgets and cancellation.

## Decomposition hints

THREAT (threat model and mechanism choice), RUNTIME (host for the chosen mechanism), CAPS (capability model and grants), FAULT (budgets, faults, validation), NUM (numeric profile inside plugins).

## Open questions

- The mechanism choice (WASM components, out-of-process native, or both by tier) must keep KNP-1 for values of record [unverified feasibility for WASM floating point]. Whether native in-process plugins are allowed at all may need a maintainer decision once the threat model is written; if so, open it from this node's THREAT child.

## Children

_None yet: status `open`._
