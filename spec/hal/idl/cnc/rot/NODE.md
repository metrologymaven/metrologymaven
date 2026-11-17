---
id: HAL.IDL.CNC.ROT
title: Rotary@1 package (angle reports only)
level: 4
parent: HAL.IDL.CNC
kind: task
status: leaf
phase: P2
size: S
owners: []
claimed_by:
satisfies: [REQ-HAL.IDL.CNC-004]
covers: []
depends_on: [HAL.IDL.BASE, HAL.IDL.UNITS, HAL.IDL.GEN.FMT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# HAL.IDL.CNC.ROT: Rotary@1 package

## Purpose

The `Rotary@1` capability package for rotary tables and indexing or continuous heads: it reports angles and nothing else. Any rotation is commanded through `CncMotion` (M1), so this package can never be a side door to motion.

## Scope

**In:**
- Operations: `describe_axes` (R0), `get_angles` (R0), `subscribe_angles` (R0 stream)
- `AngleReport { axis_id, kind: table | head_a | head_b | other, angle, t_dev, t_host, indexed: bool }`
- `Rotary-core@1` conformance profile

**Out:**
- Commanding rotation (owned by `HAL.IDL.CNC.MOTION`)
- Rotary-axis error maps and compensation (owned by `CMM` and the controller)

## Requirements

- **REQ-HAL.IDL.CNC.ROT-001** The package shall define only R0 operations; any operation of class D1 or M1 in this package shall fail the package export check.
- **REQ-HAL.IDL.CNC.ROT-002** `AngleReport.angle` shall be a `Quantity` in rad, normalised to the interval (-pi, pi], with axis ID and both timestamps.
- **REQ-HAL.IDL.CNC.ROT-003** The `Rotary-core@1` profile shall check angle normalisation, axis-ID consistency with `describe_axes`, and timestamp monotonicity per axis.

## Acceptance criteria

- **AC-1** Given `fixtures/hal/idl/cnc/rot/report_ok.json`, when validated, then it passes and round-trips identically.
- **AC-2** (error path) Given an angle of 4.0 rad, then validation fails with `angle_not_normalised`.
- **AC-3** (error path) Given a test package variant that adds a `rotate_to` operation classed M1, when exported, then the export check fails with `rotary_has_actuation`.
- **AC-4** Given the profile, then it lists the three items of REQ-HAL.IDL.CNC.ROT-003.

## Design

- **Registry commands:** none; library package.
- **Data structures:** `AngleReport`, `AxisInfo { axis_id, kind, range }`.
- **Errors:** `angle_not_normalised`, `axis_unknown`, `rotary_has_actuation`.
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-hal-idl` (module `caps::rotary`), layer L0.

## Test fixtures

Project-authored JSON under `fixtures/hal/idl/cnc/rot/` (CC-BY-4.0).

## Build prompt

> In crate `fid-hal-idl` (L0), module `caps::rotary`, define the `Rotary@1` package with R0 operations only (`describe_axes`, `get_angles`, `subscribe_angles`), `AngleReport { axis_id, kind (table|head_a|head_b|other), angle: Quantity rad normalised to (-pi, pi], t_dev, t_host, indexed }`, the `Rotary-core@1` profile (normalisation, axis-ID consistency, per-axis timestamp monotonicity), and an export check that fails `rotary_has_actuation` if any operation is D1 or M1. Implement `CapabilityPackage` for `capability-package/v1`. No panic, unwrap, expect or unsafe (CONSTITUTION §8). Tests: AC-1..AC-4.

## Open questions

- None.

## Children

_Leaf: none._
