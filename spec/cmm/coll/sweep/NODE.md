---
id: CMM.COLL.SWEEP
title: Swept volumes of the probe assembly
level: 3
parent: CMM.COLL
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-CMM.COLL-004]
covers: []
depends_on: [GEO, CMM.PROBE.MODEL, CMM.PROBE.HEAD]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# CMM.COLL.SWEEP: Swept volumes of the probe assembly

## Purpose

Builds a conservative geometric representation of the probe assembly (head, extensions, stylus, tips) from the probe configuration and head angle, and sweeps it along a linear move segment. The result is what the collision check tests against the world. It must be conservative (never smaller than the real assembly) and deterministic, so that a `CLEAR` verdict is trustworthy within the stated model limits.

## Scope

**In:**
- Assembly geometry from the probe build-up: capsules for stylus and extensions, spheres for ball tips, cylinders for disc tips, a bounding body for the head
- Pose of the assembly at a head angle and machine position
- Swept volume of the assembly over a linear segment with constant head angle
- Conservative bounds (inflation by a stated tolerance) and their documentation

**Out** (and which node owns it instead):
- Collision and clearance tests of swept volumes against models (owned by `CMM.COLL.CHECK`)
- Probe configuration and head data (owned by `CMM.PROBE`)

## Requirements

- **REQ-CMM.COLL.SWEEP-001** For a probe configuration and head angle, the system shall represent each assembly element with a primitive that contains the element's nominal geometry.
- **REQ-CMM.COLL.SWEEP-002** For a linear move segment at constant head angle, the system shall produce a swept volume that contains every position of every element along the segment.
- **REQ-CMM.COLL.SWEEP-003** The swept-volume construction shall be deterministic (class D0 or D1) and shall state its maximum over-approximation in metres in its output.
- **REQ-CMM.COLL.SWEEP-004** The system shall tag each swept primitive with the assembly element it came from (head, extension index, stylus, tip ID), so a collision can name the element involved.

## Interfaces

- **Offers:** `SweptVolume` type and constructor used by `CMM.COLL.CHECK`.
- **Consumes:** `CMM.PROBE.MODEL` build-up; `CMM.PROBE.HEAD` mount pose; `GEO` primitives.
- **Journal:** none (internal library; results are journaled by the commands that use them).

## Decomposition hints

ASSEMBLY (element primitives and pose), SEGMENT (sweep over a segment), BOUND (over-approximation bound and tests).

## Open questions

- Head-angle changes during a move (continuous articulation) are not swept here; they belong with continuous-head support (P3).

## Children

_None yet: status `open`._
