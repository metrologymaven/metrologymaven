---
id: HAL.IPP.COMP
title: Probe-compensation ownership per machine profile
level: 3
parent: HAL.IPP
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.IPP-004, REQ-HAL.IPP-005]
covers: []
depends_on: [HAL.IPP.SESS]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# HAL.IPP.COMP: Probe-compensation ownership per machine profile

## Purpose

Decides, per machine profile, who compensates probe tips: the server (default; the controller returns compensated points) or the client (the controller returns tip centres and the kernel compensates). It sets the compensation flag on every point, requests the right data from the controller, and keeps controller qualification data read-only. It is separate because the choice changes what the uncertainty and qualification workflows may claim.

## Scope

**In:**
- Machine-profile setting `compensation_owner: server | client` (A0 configuration)
- Requesting tip-centre data from the controller in client mode, compensated points in server mode
- Setting the compensation flag on every `TactileProbe` record
- Importing controller qualification results as observations with identity, never writing them back

**Out** (and which node owns it instead):
- Compensation computation in client mode (owned by `KRN` operators through `CMM` probe workflows)
- Probe qualification workflow (owned by `CMM`)

## Requirements

- **REQ-HAL.IPP.COMP-001** Each machine profile shall declare `compensation_owner` as `server` or `client`, defaulting to `server`, and changing it shall be an A0 configuration command never exposed on MCP.
- **REQ-HAL.IPP.COMP-002** In server mode, every point shall be flagged compensated; in client mode, every point shall be a tip centre flagged uncompensated.
- **REQ-HAL.IPP.COMP-003** The client shall never write qualification results, tip radii or offsets to the controller in either mode.
- **REQ-HAL.IPP.COMP-004** If the controller cannot provide the data the configured mode needs, then the session shall fail to open with `UNSUPPORTED` naming the mode.

## Interfaces

- **Offers:** compensation mode and flag to `HAL.IPP.MEAS`; machine-profile field.
- **Consumes:** `HAL.IPP.SESS`.

## Decomposition hints

Likely a single leaf once the spec cards confirm how servers expose tip-centre data.

## Open questions

- Under I++ DME the server normally owns tool data and returns compensated points [unverified; check against the VDMA 8722:2026-10 spec cards]. Until the cards exist this node cannot name the requests, so it stays open.

## Children

_None yet: status `open`._
