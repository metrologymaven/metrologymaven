---
id: GDT.ZONE.GROUP.IDS
title: Group identity, pose digest and incomplete groups
level: 4
parent: GDT.ZONE.GROUP
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.ZONE.GROUP-002, REQ-GDT.ZONE.GROUP-003]
covers: []
depends_on: [GDT.CHAR.IDENT, CORE.JRN.CANON]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
built_in: [fid-gps]
built_pr: 519
updated: 2026-11-22
---

# GDT.ZONE.GROUP.IDS: Group identity, pose digest and incomplete groups

## Purpose

Gives every shared-pose group a stable identity and makes the shared solve visible in each member's ResultRecord: a derived group ID, the digest of the shared pose, and the all-or-nothing rule for incomplete groups. It is pure bookkeeping, separate from the solve so the identity rules can be fixed and tested before any solver work.

## Scope

**In:** group ID as a UUIDv5 over the sorted member characteristic IDs and the group kind (`pattern` or `simreq`) with the source callout or group label; pose digest (SHA-256 of the RFC 8785 canonical pose with f64 as IEEE-754 hex); `GroupRef { group_id, kind, members, pose_digest }`; precheck that turns any member failure into `group_incomplete`.
**Out:** the solve (owned by `GDT.ZONE.GROUP.PATTERN`, `GDT.ZONE.GROUP.SIMREQ`); stable characteristic IDs (owned by `GDT.CHAR.IDENT`); ResultRecord building (owned by `GDT.CHAR.RECORD`).

## Requirements

- **REQ-GDT.ZONE.GROUP.IDS-001** The group ID shall be a UUIDv5 in the project namespace over the canonical JSON of {kind, label, sorted member IDs}, so the same members, kind and label always give the same ID regardless of input order.
- **REQ-GDT.ZONE.GROUP.IDS-002** The pose digest shall be the SHA-256 of the canonical JSON of the shared pose with f64 values as IEEE-754 hex strings.
- **REQ-GDT.ZONE.GROUP.IDS-003** Every member's ResultRecord payload shall carry the same `GroupRef`.
- **REQ-GDT.ZONE.GROUP.IDS-004** If any member's precheck or evaluation fails, then the module shall return `group_incomplete{member, cause}` naming the lowest failing member ID, and no member shall receive a value.
- **REQ-GDT.ZONE.GROUP.IDS-005** If a group has fewer than two members or a member appears twice, then the module shall return `bad_group{reason}`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/zone/group/ids.json` (four member IDs in two different orders), when the group ID is computed, then both orders give the ID recorded in the fixture.
- **AC-2** Given a pose with known f64 bits, then the digest equals the fixture's expected value.
- **AC-3** (error path) Given members m1..m4 where m2 and m4 fail, then `group_incomplete{m2, ...}` and no member value.
- **AC-4** (error path) Given one member, then `bad_group{too_few}`; given a duplicate, then `bad_group{duplicate}`.

## Design

- **Registry command(s):** none (internal API).
- **Data structures:** `GroupKind { Pattern, SimReq }`; `GroupRef { group_id: Uuid, kind, members: Vec<CharId>, pose_digest }`.
- **Algorithm:** sort member IDs (total order on bytes); canonical JSON; UUIDv5; SHA-256.
- **Errors:** `group_incomplete{member, cause}`, `bad_group{reason}`.
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-gps` (module `zone::group`).

## Test fixtures

Hand-written ID fixtures with expected UUIDs and digests computed by an independent script (CC0, in-repo).

## Build prompt

> In crate `fid-gps` (kernel, pure), add module `zone::group` with `GroupKind { Pattern, SimReq }`, `GroupRef { group_id, kind, members, pose_digest }`, `group_id(kind, label, members) -> Result<Uuid, GroupError>` (UUIDv5 in the project namespace over RFC 8785 canonical JSON of {kind, label, sorted member IDs}), `pose_digest(pose)` (SHA-256 of canonical JSON, f64 as IEEE-754 hex), and `all_or_nothing(results) -> Result<_, GroupError>` returning `group_incomplete{member, cause}` for the lowest failing member. Errors `bad_group{too_few|duplicate}`. Tests per the acceptance criteria. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), no std `HashMap`/`HashSet`.

## Open questions

- REQ-004 (all or nothing) awaits expert confirmation (parent open question).

## Children

_Leaf: none._
