---
id: GDT.DRF.MOB.GROUP
title: Rigid-group mobility envelopes for datum patterns
level: 4
parent: GDT.DRF.MOB
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.DRF.MOB-003, REQ-GDT.DRF.MOB-004]
covers: [PW.GDT.07]
depends_on: [GDT.DRF.MOB.REPR]
uses_artifacts: [UNC.ORS.DRF]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.DRF.MOB.GROUP: Rigid-group mobility envelopes for datum patterns

## Purpose

When several datum features act as one datum (a pattern of holes as a datum feature), they move together: the DRF has one pose, and every member's constraints must hold at that pose. This leaf combines the members' constraint sets into one envelope with a group ID. It is plain set intersection over a shared pose parameterisation; the members' own constraints come from `GDT.DRF.MOB.BOUND`.

## Scope

**In:** `combine_group(members: [MobilityEnvelope], group_id) -> MobilityEnvelope`; check that all members reference the same DRF and free-DoF set; concatenation of constraints into canonical order; union of source row IDs; the group ID on the result.
**Out:** member constraint derivation (owned by `GDT.DRF.MOB.BOUND`); which datums form a pattern (owned by the characteristic model, `GDT.CHAR.MODEL`); zone groups for toleranced features (owned by `GDT.ZONE.GROUP`).

## Requirements

- **REQ-GDT.DRF.MOB.GROUP-001** The combined envelope shall contain every member constraint unchanged, carry the group ID, and be satisfied at a pose exactly when every member envelope is satisfied at that pose.
- **REQ-GDT.DRF.MOB.GROUP-002** If members reference different DRFs or different free-DoF sets, then the function shall return `group_frame_mismatch` naming the first differing member.
- **REQ-GDT.DRF.MOB.GROUP-003** The combined envelope's digest shall not depend on member order.
- **REQ-GDT.DRF.MOB.GROUP-004** Combined envelopes shall match every ORS-DRF datum-pattern mobility case within its stated tolerance.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/drf/mob_pattern_4holes.json` (four member envelopes, each a `PointInCylinder` about its hole centre), when combined, then the result has 4 constraints, the group ID, and for 1 000 seeded poses its satisfaction equals the conjunction of the members' satisfaction.
- **AC-2** Given members in reversed order, then the digest is unchanged.
- **AC-3** (error path) Given one member on a different DRF, then `group_frame_mismatch` naming it.
- **AC-4** (ORS) Every ORS-DRF datum-pattern mobility case passes when the suite is published.

## Design

- **Registry command(s):** none (kernel function).
- **Data structures:** reuses `MobilityEnvelope`.
- **Algorithm:** verify frame and DoF equality; concatenate; canonicalise and digest with `GDT.DRF.MOB.REPR`.
- **Errors:** `group_frame_mismatch{member}`, `empty_group`.
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-gps` (module `drf::mobility::group`).

## Test fixtures

Analytic hole-pattern envelopes and a seeded pose generator (CC0); ORS-DRF suite from `UNC.ORS.DRF`.

## Build prompt

> In crate `fid-gps`, module `drf::mobility::group`, implement `combine_group(members, group_id) -> Result<MobilityEnvelope, GroupError>`: require every member to share the DRF reference and free-DoF set (else `group_frame_mismatch{member}`; empty input gives `empty_group`), concatenate all constraints unchanged, union the source row IDs, set the group ID, and canonicalise and digest with the envelope module so member order does not matter. Tests per acceptance criteria, including a 1 000-pose seeded property test that combined satisfaction equals the conjunction of member satisfaction. Constraints: CONSTITUTION §8, KNP-1, no std `HashMap`/`HashSet`.

## Open questions

- None.

## Children

_Leaf: none._
