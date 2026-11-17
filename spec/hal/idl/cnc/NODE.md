---
id: HAL.IDL.CNC
title: CMM capability packages (CncMotion, TactileProbe, Rotary)
level: 3
parent: HAL.IDL
kind: module
status: split
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.IDL-001, REQ-HAL.IDL-006]
covers: []
depends_on: [HAL.IDL.BASE, HAL.IDL.UNITS, HAL.IDL.STAMP, HAL.IDL.GEN.FMT]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# HAL.IDL.CNC: CMM capability packages (CncMotion, TactileProbe, Rotary)

## Purpose

The three capability packages that model a CNC CMM: `CncMotion` (sessions, homing, tool change, positioning, point and scan measurement, head angles, hold, abort, events), `TactileProbe` (tip selection and point events, also used by arms and 6DoF probes) and `Rotary` (angle reports). `CncMotion` follows I++ DME semantics one-to-one so the I++ client is a thin mapping. These packages are grouped because their semantics are defined together by the I++ alignment and by the motion side-effect class.

## Scope

**In:**
- `CncMotion@1`: control operations, events, queue semantics (normal and fast), hold and abort, scan streams, head angles; each annotated with the I++ clause ID it maps to
- `TactileProbe@1`: tip selection, point payload (position, direction, tip ID, tip radius, time, quality, compensated flag)
- `Rotary@1`: angle reports for tables and heads
- Semantics notes in our own words; side-effect class marking per operation
- Conformance profiles `CncMotion-core@1`, `TactileProbe-core@1`, `Rotary-core@1` (test lists)

**Out** (and which node owns it instead):
- Wire-level I++ implementation (owned by `HAL.IPP`)
- Run tickets and approval of motion (owned by `CMM`)
- Non-CMM capabilities (owned by `HAL.IDL.SENS`)
- Executing conformance tests (owned by `HAL.SIM.CONF`)

## Requirements

- **REQ-HAL.IDL.CNC-001** The `CncMotion@1` package shall define session, home, tool-change, go-to, point-measurement, scan, head-angle, hold, abort and event operations, each annotated with the I++ DME clause ID it maps to [unverified until spec cards exist].
- **REQ-HAL.IDL.CNC-002** Every `CncMotion` operation that can move an axis shall be marked side-effect class M1 in the package, so that generated registry projections never place it on MCP (CONSTITUTION §3, D-008).
- **REQ-HAL.IDL.CNC-003** The `TactileProbe@1` point payload shall carry position (m), unit direction vector, tip ID, tip radius (m), device and host timestamps, a quality indicator and a boolean stating whether the point is already probe-compensated.
- **REQ-HAL.IDL.CNC-004** The `Rotary@1` package shall define angle reports only; any rotary motion shall be expressed through `CncMotion`.
- **REQ-HAL.IDL.CNC-005** The semantics notes shall be written in the project's own words and shall cite I++ DME only by clause ID, never by quoted text (CONSTITUTION §5).
- **REQ-HAL.IDL.CNC-006** Hold and abort shall be defined on a channel that no data-plane traffic can delay, and the package shall state the maximum look-ahead queue depth a server may report.

## Interfaces

- **Offers:** `CncMotion@1`, `TactileProbe@1`, `Rotary@1` packages and their conformance profiles.
- **Consumes:** `HAL.IDL.BASE` (errors, health), `HAL.IDL.UNITS`, `HAL.IDL.STAMP`; founder-written I++ spec cards (clause IDs only).

## Decomposition hints

MOTION (`CncMotion` control operations and events), TACT (`TactileProbe` payload and tip selection), ROT (`Rotary`), SCANSTR (scan-stream payload), CARDS (clause-ID mapping table derived from spec cards).

## Split rationale

The split separates **what can be specified now** from **what waits for the I++ spec cards**. The two packages with no motion semantics, `TactileProbe@1` (TACT, also used by arms and 6DoF probes) and `Rotary@1` (ROT, angle reports only), are finished as leaves. `CncMotion@1` (MOTION) and the clause-ID table (CARDS) stay open: the operation set must be reconciled with the founder-written spec cards, which exist only from the start of P2. The scan-stream payload hint (SCANSTR) is folded into TACT, because scan results are sequences of probe points.

Alternatives considered:
- **One child per hint (MOTION, TACT, ROT, SCANSTR, CARDS).** Rejected: a separate scan-stream child would duplicate the point payload.
- **Clause IDs inside each package child.** Rejected: one table lets a reviewer check clause coverage and the no-quoted-text rule in one place.

## Open questions

- The I++ DME 3.0 specification is single-user licensed; the clause mapping waits for founder-written spec cards at the start of P2 [unverified]. `HAL.IDL.CNC.MOTION` and `HAL.IDL.CNC.CARDS` stay open until then.
- Whether head-angle changes count as motion (M1) on every head type is assumed yes; confirm with the spec cards.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| HAL.IDL.CNC.MOTION | CncMotion@1 package (operations, queue, hold and abort, events) | 001 002 006 | none |
| HAL.IDL.CNC.TACT | TactileProbe@1 package (tip selection, point and scan-point payloads) (leaf) | 003 | none |
| HAL.IDL.CNC.ROT | Rotary@1 package (angle reports only) (leaf) | 004 | none |
| HAL.IDL.CNC.CARDS | I++ DME clause-ID mapping table and own-words semantics notes | 001 005 | none |
