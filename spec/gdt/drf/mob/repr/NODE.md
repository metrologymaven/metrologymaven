---
id: GDT.DRF.MOB.REPR
title: Mobility envelope representation and digest
level: 4
parent: GDT.DRF.MOB
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.DRF.MOB-002]
covers: []
depends_on: [GDT.DRF.BUILD, GEO.FRAME, KRN.NUM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.DRF.MOB.REPR: Mobility envelope representation and digest

## Purpose

Fixes how a datum mobility envelope is written down, so that the code that builds envelopes (`GDT.DRF.MOB.BOUND`, `GDT.DRF.MOB.GROUP`), the code that searches them (`GDT.ZONE.MOBSRCH`) and the code that tests membership (`GDT.DRF.CHECK.MEMB`) share one type. An envelope is a set of rigid transforms of the DRF, given as typed geometric constraints on transformed control points and directions, evaluated exactly, plus a linearisation for convex solvers. The free and fixed degrees of freedom come from the single DoF table of `GDT.DRF.BUILD`; this leaf never redefines them.

## Scope

**In:** `Pose` parameterisation (translation in DRF axes, rotation vector through the exponential map in `fid-num`); `MobilityConstraint { PointInHalfspace, PointInBall, PointInCylinder, DirectionInCone, DofFixed }` on control points and directions given in DRF coordinates; `MobilityEnvelope { id, drf_ref, free_dofs, constraints, group_id?, source_rows }`; exact constraint evaluation `violation(constraint, pose) -> f64`; `linearize(envelope, pose) -> (A, b, cones)` about a pose for LP or SOCP use; canonical form (constraints sorted by a total key) and digest (SHA-256 over RFC 8785 canonical JSON, f64 as IEEE-754 hex); envelope ID as UUIDv5 of the digest.
**Out:** deriving constraints from MMB, LMB or rocking (owned by `GDT.DRF.MOB.BOUND`); rigid groups (owned by `GDT.DRF.MOB.GROUP`); the membership decision and result fields (owned by `GDT.DRF.CHECK`); searching the envelope (owned by `GDT.ZONE.MOBSRCH`); the DoF table (owned by `GDT.DRF.BUILD`).

## Requirements

- **REQ-GDT.DRF.MOB.REPR-001** A `MobilityEnvelope` shall consist of a DRF reference, the DRF's free-DoF set copied from `GDT.DRF.BUILD`, an ordered list of typed constraints, an optional group ID and the DMT row IDs that produced the constraints.
- **REQ-GDT.DRF.MOB.REPR-002** `violation(constraint, pose)` shall return the signed amount (metres for point constraints, radians for direction constraints) by which the exactly transformed control point or direction violates the constraint, with zero or negative meaning satisfied.
- **REQ-GDT.DRF.MOB.REPR-003** The envelope digest shall be the SHA-256 of the canonical JSON of the canonical form, and the envelope ID shall be the UUIDv5 of that digest, so equal envelopes get equal IDs regardless of constraint input order.
- **REQ-GDT.DRF.MOB.REPR-004** `linearize(envelope, pose)` shall return the first-order constraint set about the pose, and for every constraint its value at the pose shall equal `violation` at that pose bit for bit.
- **REQ-GDT.DRF.MOB.REPR-005** If a constraint acts on a DoF the DRF has already fixed, has a non-unit direction (beyond 1e-12), a negative radius, or a NaN, then construction shall fail with `constraint_on_fixed_dof`, `non_unit_direction`, `negative_radius` or `nan_input`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/drf/mob_envelope_simple.json` (a DRF with free {tx, ty} and one `PointInCylinder` of radius 0.05 mm), when `violation` is evaluated at poses (0.03, 0.04) mm and (0.06, 0) mm, then results are 0 mm and +0.01 mm within 1e-15 m.
- **AC-2** Given the same constraints in shuffled order, then digest and ID are unchanged.
- **AC-3** Given a rotation of 1e-3 rad about z and a control point 100 mm from the origin, then `violation` uses the exact rotation (differs from the linearised value by about 5e-8 m) while `linearize` reproduces `violation` at the linearisation pose exactly.
- **AC-4** (error path) Given a constraint on `tz` when the DRF fixes `tz`, then `constraint_on_fixed_dof`.
- **AC-5** (determinism) Digest bytes are identical on all CI platforms.

## Design

- **Registry command(s):** none (kernel types and functions).
- **Data structures:** as in Scope; `Pose { t: [f64; 3], omega: [f64; 3] }`.
- **Algorithm:** Rodrigues rotation via `fid-num` sin and cos; distances in closed form; Jacobians analytic; canonical sort by (kind tag, control point bits, parameters bits).
- **Errors:** as in REQ-005.
- **Determinism:** D0.
- **Performance budget:** violation of 1 000 constraints < 50 µs [unverified].
- **Crate:** `fid-gps` (module `drf::mobility`).

## Test fixtures

Analytic envelopes written in-repo (CC0).

## Build prompt

> In crate `fid-gps` (kernel), module `drf::mobility`, define `Pose { t, omega }` (translation in DRF axes; rotation vector, exact exponential map through `fid-num`), `MobilityConstraint { PointInHalfspace, PointInBall, PointInCylinder, DirectionInCone, DofFixed }` on control points and directions in DRF coordinates, and `MobilityEnvelope { id, drf_ref, free_dofs, constraints, group_id, source_rows }` where `free_dofs` is copied from the DRF's DoF table (never recomputed here). Implement `violation(constraint, pose)` (signed; <= 0 satisfied; exact transform), `linearize(envelope, pose)` (first-order set whose values at the pose equal `violation` bit for bit), canonical ordering, digest (SHA-256 of RFC 8785 canonical JSON, f64 as IEEE-754 hex) and ID (UUIDv5 of the digest). Reject constraints on fixed DoFs, non-unit directions, negative radii and NaN. Tests per acceptance criteria. Constraints: CONSTITUTION §8, KNP-1 (no FMA, `fid-num` transcendentals, fixed-order reductions, no std `HashMap`/`HashSet`, NaN is an error).

## Open questions

- If `GDT.DRF.MOB.BOUND` finds that some boundary needs a non-convex constraint kind, it is added here as a new variant with a `non_convex` marker, never approximated silently.

## Children

_Leaf: none._
