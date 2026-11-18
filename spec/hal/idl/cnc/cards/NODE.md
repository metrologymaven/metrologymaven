---
id: HAL.IDL.CNC.CARDS
title: I++ DME clause-ID mapping table and own-words semantics notes
level: 4
parent: HAL.IDL.CNC
kind: task
status: open
phase: P2
size: L
owners: []
claimed_by:
needs_expert: cmm-control
satisfies: [REQ-HAL.IDL.CNC-001, REQ-HAL.IDL.CNC-005]
covers: []
depends_on: []
uses_artifacts: [HAL.IDL.CNC.MOTION]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# HAL.IDL.CNC.CARDS: I++ DME clause-ID mapping table and own-words semantics notes

## Purpose

The mapping from every `CncMotion@1`, `TactileProbe@1` and `Rotary@1` operation and event to the I++ DME clause it corresponds to, by clause ID only, plus the semantics notes written in the project's own words. It is derived from founder-written spec cards because the I++ DME specification is single-user licensed; no text from it may enter the repository.

## Scope

**In:**
- `ipp-clause-map/1` table: package, operation or event, I++ clause ID, mapping kind (`exact | partial | none`), card reference
- Semantics notes per operation in our own words
- A CI check that every operation of the three packages has a row, and that no note contains quoted specification text (similarity check against nothing stored; reviewers confirm)

**Out:**
- The specification text itself (never stored; CONSTITUTION §5)
- Package definitions (owned by `HAL.IDL.CNC.MOTION`, `HAL.IDL.CNC.TACT`, `HAL.IDL.CNC.ROT`)
- Writing the spec cards (founder task outside the spec tree)

## Requirements

- **REQ-HAL.IDL.CNC.CARDS-001** The mapping table shall hold one row per operation and event of the three CMM packages, naming the I++ DME clause ID and mapping kind [unverified until spec cards exist].
- **REQ-HAL.IDL.CNC.CARDS-002** Semantics notes shall be written in the project's own words and shall cite I++ DME only by clause ID.
- **REQ-HAL.IDL.CNC.CARDS-003** If an operation of the three packages has no row, then the package export check shall fail naming the operation.

## Interfaces

- **Offers:** `ipp-clause-map/1` table (artifact read by `HAL.IDL.CNC.MOTION`, `HAL.IPP` and `HAL.SIM.IPPSIM`).
- **Consumes:** operation lists exported by the three packages; founder-written spec cards.

## Decomposition hints

Likely one leaf (table plus check) once the spec cards exist.

## Open questions

- Blocked on content, not a decision: the founder-written spec cards for VDMA 8722 (I++ DME) are expected at the start of P2 [unverified]. Until then this node stays open.

## Children

_None yet: status `open`._
