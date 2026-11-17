---
id: HAL.SIM.ERR
title: L2 seeded error and noise injection
level: 3
parent: HAL.SIM
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.SIM-002, REQ-HAL.SIM-003]
covers: []
depends_on: [HAL.SIM.IPPSIM, HAL.SIM.WORK]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# HAL.SIM.ERR: L2 seeded error and noise injection

## Purpose

Fidelity level L2: the I++ simulator (and, where meaningful, the other simulators) with seeded geometry errors, probing noise, thermal drift, latency, link drops, tool-change failures, observed safety stops and client death. It serves robustness tests of the client and Motion Executive and produces Monte Carlo corpora for uncertainty validation. It is separate because its models and seeds must be reproducible and reviewed as an uncertainty input.

## Scope

**In:**
- Geometry error model driven by an instrument error parameter set (IEMPS-style) [unverified mapping]
- Probing noise, thermal drift over time, latency and jitter
- Fault injection: link drops mid-move, tool-change failure, observed safety stop, client death, out-of-order events
- A seed per run and a counter-based random generator; per-run manifest of injected errors
- Reuse of the same injection hooks by synthetic simulators (pose, scan, gauge)

**Out** (and which node owns it instead):
- The error parameter sets themselves and uncertainty evaluation (owned by `UNC`)
- Ideal kinematics and protocol (owned by `HAL.SIM.IPPSIM`)

## Requirements

- **REQ-HAL.SIM.ERR-001** Given the same seed, scenario and client inputs, an L2 run shall produce bit-identical protocol output and injected-error manifest on the same platform (D2).
- **REQ-HAL.SIM.ERR-002** Every injected error shall be recorded in a run manifest with its type, time and parameters, so a test can assert on what was injected.
- **REQ-HAL.SIM.ERR-003** The geometry error model shall accept a parameter set by digest and apply it to every reported point.
- **REQ-HAL.SIM.ERR-004** Fault injection shall support at least: link drop mid-move, tool-change failure, observed safety stop, event reordering and client-death detection, each schedulable by time or by command count.
- **REQ-HAL.SIM.ERR-005** Random numbers shall come only from the seeded counter-based generator of `fid-num`, never from an OS source.

## Interfaces

- **Offers:** L2 mode for `fid-ipp-sim`; injection hooks for synthetic simulators; run manifests.
- **Consumes:** `HAL.SIM.IPPSIM`, `HAL.SIM.WORK`; parameter sets from `UNC.INST` by digest.

## Decomposition hints

GEOM (geometry error model), NOISE (noise, drift, latency), FAULT (fault schedule), MANIF (run manifests; likely a leaf).

## Open questions

- Which ISO/TS 15530-4 inputs the parameter-set format must carry is for `UNC.INST` to settle [unverified].

## Children

_None yet: status `open`._
