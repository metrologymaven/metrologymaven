---
id: CMM.EXEC.REQ
title: Run and hold requests and the request queue
level: 3
parent: CMM.EXEC
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-CMM.EXEC-001, REQ-CMM.EXEC-003]
covers: []
depends_on: [CORE, AUTO]
cli: [fid cmm request-run, fid cmm request-hold, fid cmm list-run-requests]
mcp: [cmm_request_hold, cmm_list_run_requests]
journal: [cmm.request_run@1, cmm.request_hold@1]
updated: 2026-11-16
---

# CMM.EXEC.REQ: Run and hold requests and the request queue

## Purpose

Owns the two ways anything outside the machine-side host can ask for motion-related action: the **run request**, an inert queued record that causes no device I/O until the Approval Station pulls it and a ticket is approved, and the **hold request**, the only motion-related effect reachable over MCP. Under D-008, agents may not file run requests over MCP; their only motion-related tools are the hold request (scope `run:hold`) and read-only status.

## Scope

**In:**
- `RunRequest` record (program, machine, setup and probe digests requested, requester principal and client, requested scope) and its queue
- Station pull of queued requests over an outbound, mutually authenticated channel (the machine side never listens on a network socket)
- Rate limits per principal and client
- `cmm.request_run@1` (M1, request only; CLI and GUI by a human principal; **never projected to MCP**)
- `cmm.request_hold@1` (MCP-exposed as `cmm_request_hold`; authenticated, `run:hold` scope, rate-limited, journaled, documented as advisory)
- Read-only queue status (`cmm.list_run_requests@1`, R0)

**Out** (and which node owns it instead):
- Issuing tickets (owned by `CMM.EXEC.TICKET`)
- Executing a hold on the controller (owned by `CMM.EXEC.DRIVE`)
- The registry projection machinery (owned by `CORE` and `AUTO.MCP`)

## Requirements

- **REQ-CMM.EXEC.REQ-001** The system shall register `cmm.request_run@1` with side-effect class M1 so that the registry generates no MCP tool for it, and the MCP projection test shall fail if any MCP tool can create a run request.
- **REQ-CMM.EXEC.REQ-002** If a run request arrives from an agent principal by any gateway, then the system shall reject it with `RUN_REQUEST_AGENT_FORBIDDEN` and journal the rejection (D-008).
- **REQ-CMM.EXEC.REQ-003** A run request shall be an inert queued record: creating, listing or expiring it shall cause no device I/O and no communication with the Motion Executive other than the station's pull.
- **REQ-CMM.EXEC.REQ-004** The system shall expose `cmm.request_hold@1` over MCP only to principals holding the `run:hold` scope, rate-limited per principal (default 6 per minute [unverified]), journaled, and documented as advisory (a hold may not take effect if the controller link is lost).
- **REQ-CMM.EXEC.REQ-005** The system shall expose no MCP tool that starts, continues or resumes motion, and the generated MCP manifest for `cmm.*` shall contain only R0 or R1 tools plus `cmm_request_hold`, apart from simulation (I1) and planning or draft-writing tools (W1, W2).
- **REQ-CMM.EXEC.REQ-006** The system shall rate-limit run requests per principal and client (default 10 per hour [unverified]) to blunt approval-fatigue flooding.

## Interfaces

- **Offers:** `cmm.request_run@1` (M1, never on MCP), `cmm.request_hold@1` (MCP: `cmm_request_hold`), `cmm.list_run_requests@1` (R0); RunRequest, HoldRequest types; the station pull channel contract.
- **Consumes:** `CORE` principals, scopes, journal; `AUTO` released program references.
- **Journal:** `cmm.request_run@1`, `cmm.request_hold@1`.

## Decomposition hints

RUNREQ (run request record and queue), HOLDREQ (hold request), PULL (station pull channel), PROJ (MCP projection tests).

## Open questions

- The side-effect class of `cmm.request_hold@1` is not in CONSTITUTION §3.1. It writes a request record (W1-like) but is reachable by agents under `run:hold`; the leaf should confirm the class, possibly through a D-007 follow-up decision.
- Rate-limit defaults are placeholders.

## Children

_None yet: status `open`._
