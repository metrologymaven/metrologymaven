---
id: GDT.DRF.CHECK.MEMB
title: Membership test of a transform in a mobility envelope
level: 4
parent: GDT.DRF.CHECK
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.DRF.CHECK-001]
covers: []
depends_on: [GDT.DRF.MOB.REPR, GDT.DRF.CHECK.FIELDS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.DRF.CHECK.MEMB: Membership test of a transform in a mobility envelope

## Purpose

Decides whether the transform an evaluation used lies inside a general mobility envelope (datums at MMB or LMB, groups): evaluate every constraint of the envelope at that transform with the exact `violation` function and accept when no violation exceeds the tolerance. It implements the `MembershipTest` trait of `GDT.DRF.CHECK.FIELDS`, so P1a code paths are unchanged when mobility arrives in P1b.

## Scope

**In:** `MembershipTest` implementation over `MobilityEnvelope`; conversion of the transform used to the envelope's `Pose`; per-constraint violation with tolerance 1e-9 m for point constraints and 1e-9 rad for direction constraints [unverified]; reporting the worst constraint index and its violation.
**Out:** envelope type and `violation` (owned by `GDT.DRF.MOB.REPR`); result fields (owned by `GDT.DRF.CHECK.FIELDS`); envelope search (owned by `GDT.ZONE.MOBSRCH`).

## Requirements

- **REQ-GDT.DRF.CHECK.MEMB-001** The test shall return within-envelope true exactly when every constraint's violation at the transform used is at most 1e-9 m (point constraints) or 1e-9 rad (direction constraints) and every fixed DoF matches the DRF within the same tolerances.
- **REQ-GDT.DRF.CHECK.MEMB-002** The test shall report the index and value of the largest violation, lowest index on ties.
- **REQ-GDT.DRF.CHECK.MEMB-003** If the transform used references a different DRF than the envelope, then the test shall return `envelope_frame_mismatch`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/drf/mob_envelope_simple.json` and a transform at (0.03, 0.04) mm, when tested, then within is true; at (0.06, 0) mm, then false with worst violation +0.01 mm at index 0.
- **AC-2** Given a transform violating a constraint by exactly 1e-9 m, then within is true; by 2e-9 m, then false.
- **AC-3** (error path) Given a transform on another DRF, then `envelope_frame_mismatch`.
- **AC-4** (determinism) Results are bit-identical across CI platforms.

## Design

- **Registry command(s):** none (implements `MembershipTest`).
- **Data structures:** `Membership { within: bool, worst_index: Option<u32>, worst_violation: f64 }`.
- **Algorithm:** pose from transform (rotation vector through `fid-num`), loop constraints in canonical order, total-order max.
- **Errors:** `envelope_frame_mismatch`, `nan_input`.
- **Determinism:** D0.
- **Performance budget:** 1 000 constraints < 50 µs [unverified].
- **Crate:** `fid-gps` (module `drf::check::membership`).

## Test fixtures

Fixtures of `GDT.DRF.MOB.REPR` (CC0).

## Build prompt

> In crate `fid-gps`, module `drf::check::membership`, implement the `MembershipTest` trait for `MobilityEnvelope`: convert the transform used to a `Pose`, evaluate every constraint's exact `violation` in canonical order, and return `within = true` exactly when all point violations are <= 1e-9 m, all direction violations <= 1e-9 rad and fixed DoFs match within those tolerances; report the worst constraint index and value (lowest index on ties). Different DRF gives `envelope_frame_mismatch`. Tests per acceptance criteria. Constraints: CONSTITUTION §8, KNP-1 (total-order max, `fid-num` transcendentals, NaN is an error), no std `HashMap`/`HashSet`.

## Open questions

- None.

## Children

_Leaf: none._
