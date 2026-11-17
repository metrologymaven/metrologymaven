---
id: CMM.SIM.HITS
title: Simulated probe hits and scan data
level: 3
parent: CMM.SIM
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-CMM.SIM-003]
covers: []
depends_on: [GEO, HAL.ACQ, CMM.COLL.FIXT]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# CMM.SIM.HITS: Simulated probe hits and scan data

## Purpose

Produces the observations a simulation would record: for each probe move, the contact point of the tip on the nominal part (tip-centre and surface point, with the surface normal), and for scan moves a simulated point sequence at the strategy's density. Every such observation carries the indelible `simulated` provenance flag, so it can never be mistaken for, or mixed into, measured data of record.

## Scope

**In:**
- Contact computation of a tip sphere moving along a probe move against the nominal part model
- Simulated scan points along scan moves
- Optional deterministic noise (seeded) for downstream testing, off by default
- Provenance stamping with `simulated` through the HAL acquisition stamp

**Out** (and which node owns it instead):
- Error-injected uncertainty simulation (owned by `UNC`)
- Simulator kinematics (owned by `HAL.SIM`)
- Provenance stamp format (owned by `HAL.ACQ`)

## Requirements

- **REQ-CMM.SIM.HITS-001** For each probe move whose tip path meets the nominal part within the probing zone, the system shall produce one simulated hit with tip-centre position, surface point and surface normal in the machine frame.
- **REQ-CMM.SIM.HITS-002** If a probe move reaches its search limit without contact, then the system shall record a `NO_CONTACT` event for that move instead of a hit.
- **REQ-CMM.SIM.HITS-003** Every simulated observation shall carry the `simulated` provenance flag, and no command shall be able to clear it.
- **REQ-CMM.SIM.HITS-004** If noise is enabled, then the system shall draw it from the seeded counter-based RNG and record the seed and noise model; with noise off, output shall be class D0.

## Interfaces

- **Offers:** simulated hit and scan generators used by `CMM.SIM.RUN`; SimulatedObservation type.
- **Consumes:** `GEO` nominal part and distance queries; `HAL.ACQ` provenance stamp; `CMM.COLL.FIXT` part placement.
- **Journal:** none directly (observations are journaled by `cmm.simulate_program@1`).

## Decomposition hints

TOUCH (discrete hits), SCAN (simulated scans), NOISE (seeded noise), STAMP (provenance).

## Open questions

- None.

## Children

_None yet: status `open`._
