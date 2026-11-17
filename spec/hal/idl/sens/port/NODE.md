---
id: HAL.IDL.SENS.PORT
title: PortablePose@1 package (6DoF pose stream)
level: 4
parent: HAL.IDL.SENS
kind: feature
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.IDL.SENS-001, REQ-HAL.IDL.SENS-003]
covers: []
depends_on: [HAL.IDL.SENS.STREAM, HAL.IDL.BASE, HAL.IDL.UNITS, HAL.IDL.GEN.FMT]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# HAL.IDL.SENS.PORT: PortablePose@1 package

## Purpose

The `PortablePose@1` package for laser trackers, articulated arms and optical 6DoF systems: a stream of poses with covariance, measurement triggers, reflector or target state and weather inputs. Tactile points from these devices reuse `TactileProbe@1` (`HAL.IDL.CNC.TACT`).

## Scope

**In:**
- Pose batch: position (m), orientation (unit quaternion), 6x6 covariance in SI units, frame, timestamps, target ID
- Trigger modes (single, stable-point, time, distance) and trigger events
- Reflector or target state (locked, lost, searching) as events
- Weather inputs (temperature, pressure, humidity) as `Environment@1`-compatible readings or references
- `PortablePose-core@1` conformance profile

**Out:**
- Tactile point payloads (owned by `HAL.IDL.CNC.TACT`)
- Leapfrog and station bookkeeping (owned by `HAL.LIVE.LEAP` and `LVM`)
- Network adjustment (owned by `LVM`)

## Requirements

- **REQ-HAL.IDL.SENS.PORT-001** A pose batch entry shall carry position in m, orientation as a unit quaternion, a symmetric positive semi-definite 6x6 covariance in m^2, m rad and rad^2 blocks, the frame it is expressed in, and `t_dev` and `t_host`.
- **REQ-HAL.IDL.SENS.PORT-002** If a covariance is not symmetric within 1e-12 relative or has a negative eigenvalue below -1e-18, then validation shall fail with `covariance_invalid` [unverified tolerances].
- **REQ-HAL.IDL.SENS.PORT-003** Target-lost and target-locked transitions shall be events on the stream, and pose entries shall not be emitted while the target state is `lost`.
- **REQ-HAL.IDL.SENS.PORT-004** Weather inputs used by the device for refractive-index correction shall be reported with every batch as values or references, so the correction is traceable.

## Interfaces

- **Offers:** `PortablePose@1` package and `PortablePose-core@1` profile.
- **Consumes:** `HAL.IDL.SENS.STREAM`, `HAL.IDL.BASE`, `HAL.IDL.UNITS`, `HAL.IDL.GEN.FMT`.

## Decomposition hints

Likely one M leaf once the first tracker and arm drivers (P3) confirm trigger modes and weather reporting.

## Open questions

- Whether an open "I++ for portables" standard should replace this package later (concept doc 08 §10) is out of scope until P4.
- Covariance conventions differ between vendors [unverified]; the first tracker integration should confirm the block order.

## Children

_None yet: status `open`._
