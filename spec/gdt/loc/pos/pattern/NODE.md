---
id: GDT.LOC.POS.PATTERN
title: Pattern position with one shared pose
level: 4
parent: GDT.LOC.POS
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.LOC.POS-001, REQ-GDT.LOC.POS-003, REQ-GDT.LOC.POS-004]
covers: [PW.GDT.04]
depends_on: [GDT.LOC.POS.RFS, GDT.LOC.POS.MMC, GDT.ZONE.GROUP]
uses_artifacts: [UNC.ORS.GDT]
side_effect: R1
determinism: D3
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.LOC.POS.PATTERN: Pattern position with one shared pose

## Purpose

A position callout on a pattern of holes or pins (for example 4X) is evaluated with one shared pose of the whole zone framework: each member's zone sits at its true position relative to the others, and the framework moves only within the DRF's free DoFs. This leaf builds the group problem from the members' derived elements and true positions, asks `GDT.ZONE.GROUP` for the shared solve, and returns one position value per member, each referencing the group ID and the shared pose digest.

## Scope

**In:** group formation from the pattern count in the characteristic; per-member derived elements and true positions; RFS and material-condition members (each member's own bonus where the row says so); one `solve_group` call; per-member `PosValue` with group ID and pose digest; whole-group failure on a missing member.
**Out:** the shared-pose solver (owned by `GDT.ZONE.GROUP`); composite tiers (owned by `GDT.ZONE.COMP`); simultaneous requirements across callouts (owned by `GDT.ZONE.GROUP`, P1b).

## Requirements

- **REQ-GDT.LOC.POS.PATTERN-001** The function shall evaluate all members of a pattern with one shared pose from `solve_group` and report each member's position value at that pose.
- **REQ-GDT.LOC.POS.PATTERN-002** Each member's result shall reference the group ID and the shared pose digest.
- **REQ-GDT.LOC.POS.PATTERN-003** If any member has no derived element (missing points or failed derivation), then the function shall return `group_incomplete{member}` and no member value.
- **REQ-GDT.LOC.POS.PATTERN-004** For a fully constrained DRF, each member's value shall equal its single-feature RFS value bit for bit; values shall match every ORS-GDT pattern position case in the P1a subset within its stated tolerance.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/loc/pos_pattern_4x.json` (four holes, DRF A|B|C fully constrained, planted offsets), when evaluated, then each member's value equals `position_rfs` for that member bit for bit and all reference one group ID.
- **AC-2** Given the same pattern with only datum A (framework free to translate and rotate in plane) and a group-solver stub, then all members are evaluated at the stub's shared pose and carry its digest.
- **AC-3** (error path) Given member 3 without points, then `group_incomplete{member: 3}` and no values.
- **AC-4** (ORS) Every P1a ORS-GDT pattern position case passes when the solver and suite exist.

## Design

- **Registry command(s):** none (kernel function `position_pattern(members, drf, chain, solver) -> [PosValue]`).
- **Data structures:** `PatternMember { label, derived, nominal }`; results reuse `PosValue` plus `group_id`, `pose_digest`.
- **Algorithm:** derive each member; build zone shapes at true positions; `solve_group`; per-member usage at the shared pose.
- **Errors:** `group_incomplete{member}`, pass-through solver errors.
- **Determinism:** D3 when the group solver iterates; D0 at a fixed pose.
- **Performance budget:** 100 members fixed pose < 5 ms [unverified].
- **Crate:** `fid-gps` (module `loc::position::pattern`).

## Test fixtures

Analytic hole patterns (CC0); ORS-GDT suite from `UNC.ORS.GDT`.

## Build prompt

> In crate `fid-gps`, module `loc::position::pattern`, implement `position_pattern(members, drf, chain, solver)`: derive every member's element (fail the whole group with `group_incomplete{member}` on any failure), place each member's zone at its true position, call the shared-pose group solver once, and return each member's position value at the shared pose with the group ID and pose digest; members at MMC use the material-condition path. With a fully constrained DRF the result must equal single-feature RFS values bit for bit. Tests per acceptance criteria with a solver stub. Constraints: CONSTITUTION §8, KNP-1, no std `HashMap`/`HashSet`.

## Open questions

- Inherited from `GDT.ZONE.GROUP`: whether a failed member fails the whole group is an expert-confirmed behaviour; this leaf follows that node's REQ-003.

## Children

_Leaf: none._
