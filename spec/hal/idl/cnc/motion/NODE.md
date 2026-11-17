---
id: HAL.IDL.CNC.MOTION
title: CncMotion@1 package (operations, queue, hold and abort, events)
level: 4
parent: HAL.IDL.CNC
kind: feature
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.IDL.CNC-001, REQ-HAL.IDL.CNC-002, REQ-HAL.IDL.CNC-006]
covers: []
depends_on: [HAL.IDL.BASE, HAL.IDL.UNITS, HAL.IDL.CNC.TACT, HAL.IDL.GEN.FMT]
uses_artifacts: [HAL.IDL.CNC.CARDS]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# HAL.IDL.CNC.MOTION: CncMotion@1 package

## Purpose

The `CncMotion@1` capability package: the operations, queue semantics, hold and abort channel, events and scan operations of a CNC CMM, defined so that the I++ client (`HAL.IPP`) is a thin mapping and the simulator (`HAL.SIM.IPPSIM`) implements the same contract. Every operation that can move an axis is marked M1 in the package itself, so no generated projection can ever place it on MCP.

## Scope

**In:**
- Operations: session start and end, home, tool change, go-to, point measurement, scan (along a curve, on a circle, unknown contour), head-angle set, hold, resume, abort, event subscription, queue status
- Queue semantics: normal and fast queues, maximum look-ahead depth reported by the server
- Hold and abort on a control channel that data-plane traffic cannot delay
- Event types (motion started, completed, error, hold acknowledged, safety stop observed) and their payloads
- Side-effect class per operation (M1 for anything that can move an axis or head; D1 otherwise; R0 for status)
- Scan results as `TactileProbe@1` scan-point batches
- `CncMotion-core@1` conformance profile (test list)

**Out:**
- Tip selection and point payloads (owned by `HAL.IDL.CNC.TACT`)
- Angle reports (owned by `HAL.IDL.CNC.ROT`)
- I++ clause-ID annotation (owned by `HAL.IDL.CNC.CARDS`)
- Wire protocol (owned by `HAL.IPP`); run tickets and approval (owned by `CMM`)

## Requirements

- **REQ-HAL.IDL.CNC.MOTION-001** The package shall define the operations listed in Scope, each with typed request, response, allowed `ErrorCode`s and a side-effect class.
- **REQ-HAL.IDL.CNC.MOTION-002** Every operation that can move a machine axis or a probe head (home, tool change, go-to, point measurement, scan, head-angle set, resume) shall be marked M1, and generated MCP projections shall contain none of them.
- **REQ-HAL.IDL.CNC.MOTION-003** Hold and abort shall be defined on a control channel separate from the data plane, and the conformance profile shall require hold acknowledgement within a latency the server declares in describe.
- **REQ-HAL.IDL.CNC.MOTION-004** The package shall define a describe field for the maximum look-ahead queue depth, and a server that queues more operations than it declared shall fail `CncMotion-core@1`.
- **REQ-HAL.IDL.CNC.MOTION-005** Each operation shall carry the I++ DME clause ID from the `HAL.IDL.CNC.CARDS` mapping table, and an operation without a mapping entry shall fail the package export [unverified until spec cards exist].
- **REQ-HAL.IDL.CNC.MOTION-006** All positions, distances, speeds and angles shall be `Quantity` values in m, m/s and rad, in a declared machine frame.

## Interfaces

- **Offers:** `CncMotion@1` package and `CncMotion-core@1` profile.
- **Consumes:** `HAL.IDL.BASE` (errors, health, configuration), `HAL.IDL.UNITS`, `HAL.IDL.CNC.TACT` scan-point batches, `HAL.IDL.GEN.FMT`; clause table artifact from `HAL.IDL.CNC.CARDS`.

## Decomposition hints

OPS (operation set and classes; a leaf once CARDS has the clause list), QUEUE (queue and look-ahead semantics), HALT (hold, resume and abort channel), EVT (events), PROF (`CncMotion-core@1` items). It may also finish as one M leaf once the spec cards confirm the operation set.

## Open questions

- Left open on purpose: the operation set must be reconciled with the founder-written I++ DME spec cards (P2 start) before a buildable leaf can name each operation's exact semantics [unverified].
- Head-angle changes are assumed M1 on every head type; confirm with the spec cards.

## Children

_None yet: status `open`._
