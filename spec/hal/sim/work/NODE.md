---
id: HAL.SIM.WORK
title: Virtual workpiece, stylus contact model and collision detection
level: 3
parent: HAL.SIM
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.SIM-004]
covers: []
depends_on: [GEO]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# HAL.SIM.WORK: Virtual workpiece, stylus contact model and collision detection

## Purpose

The simulated world the I++ simulator and the tactile simulators measure: a workpiece made of primitives or CAD, placed with an offset that alignment must find, a stylus-sphere contact model, and collision detection for any unexpected contact. It is separate because it is the geometric core shared by the I++ simulator, the arm simulator and the error-injection layer.

## Scope

**In:**
- Workpiece description: primitives (plane, sphere, cylinder, cone, box) and, later, tessellated CAD
- Hidden placement offset (rigid transform) configured per scenario
- Contact model: stylus sphere against surfaces along a probing direction; returned point, normal and tip identity
- Collision detection for the stylus, shaft and probe body along a linear move
- Fixtures and clamps as obstacles

**Out** (and which node owns it instead):
- Geometry kernels for exact CAD (owned by `GEO`; consumed here)
- Path planning and collision avoidance for programs (owned by `CMM`)
- Error injection on contact results (owned by `HAL.SIM.ERR`)

## Requirements

- **REQ-HAL.SIM.WORK-001** When a probing move along direction d reaches the workpiece, the contact model shall return the stylus-centre position at first contact and the surface normal, within 1e-9 m of the analytic value for primitive surfaces.
- **REQ-HAL.SIM.WORK-002** When any part of the stylus, shaft or probe body intersects the workpiece or a fixture during a move that is not a commanded measurement approach, the model shall report a collision with the first contact position and stop the move there.
- **REQ-HAL.SIM.WORK-003** The workpiece placement offset shall be hidden from the client and applied consistently to every query, so that only alignment can recover it.
- **REQ-HAL.SIM.WORK-004** Contact and collision queries shall be deterministic (D0) for primitive workpieces.

## Interfaces

- **Offers:** `contact(move) -> Contact | NoContact | Collision`; workpiece scenario format.
- **Consumes:** `GEO` primitives and, later, tessellated CAD.

## Decomposition hints

PRIM (primitive workpieces and analytic contact; likely a leaf), COLL (swept-volume collision for stylus, shaft and body), CAD (tessellated CAD workpieces), PLACE (hidden placement and scenario format).

## Open questions

- Whether the arm and 6DoF-probe simulators reuse this model directly or a simplified one is left to `HAL.SIM.PORT`.

## Children

_None yet: status `open`._
