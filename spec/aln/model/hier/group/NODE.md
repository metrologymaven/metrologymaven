---
id: ALN.MODEL.HIER.GROUP
title: Alignment groups
level: 4
parent: ALN.MODEL.HIER
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-ALN.MODEL.HIER-003]
covers: [PW.ALN.08]
depends_on: [ALN.MODEL.ELEM, ALN.MODEL.HIER.CHAIN, GEO.FRAME.APPLY, CORE.DEP.EDGE]
side_effect: [W2, W2]
determinism: D0
cli: [fid align create-group, fid align edit-group]
mcp: [align_create_group, align_edit_group]
journal: [align.create_group@1, align.edit_group@1]
updated: 2026-11-17
---

# ALN.MODEL.HIER.GROUP: Alignment groups

## Purpose

An alignment group (PolyWorks-style) holds several data elements that move together: one alignment's composed transform places every member, and a solver can take the group as its measured input so one transform is computed for all members at once.

## Scope

**In:** the `AlignmentGroup` element; `align.create_group@1` and `align.edit_group@1` (add, remove, set alignment, rename); exclusive membership; frame consistency of members; dependency edges so members' placements follow the group's alignment.
**Out:** the chain and composition (`ALN.MODEL.HIER.CHAIN`); solvers that consume a group as input (`ALN.BFIT`, `ALN.RPS`, `ALN.DATUM`); deletion of the group's alignment (`ALN.MODEL.HIER.DEL`).

## Requirements

- **REQ-ALN.MODEL.HIER.GROUP-001** When `align.create_group` commits with a name, a list of data elements and an optional alignment, the system shall create one group whose members are stored sorted by element ID.
- **REQ-ALN.MODEL.HIER.GROUP-002** While a group names an alignment, every member's placement shall be that alignment's `compose_to_root` transform, and a CORE.DEP edge from the alignment to the group shall mark member-dependent results `outdated` when it changes.
- **REQ-ALN.MODEL.HIER.GROUP-003** If an element to be added already belongs to another group, then the command shall fail with `element_in_other_group` naming the element and its group, and journal nothing.
- **REQ-ALN.MODEL.HIER.GROUP-004** If the members are not all in the same GEO frame, then the command shall fail with `frame_mismatch` (GEO.FRAME.APPLY guard) and journal nothing.
- **REQ-ALN.MODEL.HIER.GROUP-005** `align.edit_group` shall accept exactly one operation per call (`add`, `remove`, `set_alignment`, `rename`), and removing the last member shall fail with `empty_group`.
- **REQ-ALN.MODEL.HIER.GROUP-006** When a group is passed as a solver's measured input, the solver shall receive the members' points concatenated in member order, and the alignment shall record the group ID and its membership digest.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/aln/model/hier/two_scans.fidproj` (two clouds and alignment A), when `fid align create-group --name G --members s1,s2 --alignment A --json` runs, then group G exists with members [s1, s2] and both clouds are placed by A's composed transform.
- **AC-2** (error path) Given s1 already in G, when another group is created with s1, then it fails with `element_in_other_group{s1, G}`.
- **AC-3** Given G, when A is recomputed, then results depending on s1 and s2 are marked `outdated`.
- **AC-4** (error path) Given `edit-group --remove` on the last member, then it fails with `empty_group`.
- **AC-5** (replay) Exact replay of AC-1 gives `IDENTICAL`.

## Design

- **Registry commands:** `align.create_group@1` (W2; params `name`, `members: [ElementId]`, `alignment?`); `align.edit_group@1` (W2; params `group`, `op: add{members} | remove{members} | set_alignment{alignment | none} | rename{name}`). Results: group ID and membership digest.
- **Data structures:** `AlignmentGroup { id, name, members: Vec<ElementId> sorted, alignment: Option<AlignmentId>, membership_digest }`.
- **Errors:** `element_in_other_group`, `frame_mismatch`, `empty_group`, `not_found`, `name_exists`.
- **Determinism:** D0.
- **Performance budget:** edit of a 1,000-member group in under 5 ms [unverified until measured].
- **Crate:** `fid-align` (provisional name).

## Test fixtures

`two_scans.fidproj` (small synthetic clouds, CC0).

## Build prompt

> In crate `fid-align`, add the `AlignmentGroup` element (`id`, `name`, sorted `members`, optional `alignment`, `membership_digest`). Register `align.create_group@1` and `align.edit_group@1` (both W2; CLI `fid align create-group` / `edit-group`; MCP `align_create_group` / `align_edit_group`). Enforce exclusive membership (`element_in_other_group`), one GEO frame for all members (GEO.FRAME.APPLY guard → `frame_mismatch`), one operation per edit, and `empty_group`. Place members by the group alignment's `compose_to_root` (ALN.MODEL.HIER.CHAIN) and add a CORE.DEP edge alignment → group so dependents go `outdated` when it changes. When a solver takes a group as measured input, concatenate members' points in member order and record group ID and membership digest. Errors journal nothing. Tests: AC-1..AC-5. Constraints: CONSTITUTION §3, §8 (no `unwrap`/`expect`/`panic`/`unsafe`).

## Open questions

- PolyWorks' group semantics are assumed from public descriptions [unverified].

## Children

_Leaf: none._
