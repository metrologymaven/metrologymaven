---
id: CORE.PRJ.TREE.MODEL
title: Element record, states, state view and type registry
level: 4
parent: CORE.PRJ.TREE
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.PRJ.TREE-001, REQ-CORE.PRJ.TREE-003, REQ-CORE.PRJ.TREE-004, REQ-CORE.PRJ.TREE-007]
covers: [GOM.PRJ.03, PW.PRJ.03]
depends_on: [CORE.PRJ.CONT.ID, CORE.PRJ.CONT.HIER, CORE.BUS.PIPE.HANDLER, CORE.REG.DEF]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.PRJ.TREE.MODEL: Element record, states, state view and type registry

## Purpose

The in-memory document: every element with its ID, registered type, parent group, name, per-stage slots (each with a value digest and one of the four states), visibility flag and metadata slot. It implements the bus's read-only `StateView`, applies committed events to produce the next state, and owns the `ElementTypeRegistry` through which other domains add element types. The tree is what the element explorer shows and what the dependency graph walks.

## Scope

**In:** `Element` record; `ElementState` (`current | outdated | failed | not_computed`) and the allowed transitions; per-stage slots with an implicit default stage when a project has no explicit stages; `ElementTypeRegistry::register`; the `StateView` implementation; event application (`apply(event)`), deterministic and total; untrusted names.
**Out:** group, move, rename and visibility commands (`CORE.PRJ.TREE.GROUP`); queries (`CORE.PRJ.TREE.QUERY`); tags and metadata commands (`CORE.PRJ.META`); setting states by propagation and recompute (`CORE.DEP.PROP`, `CORE.DEP.RECOMP`); payload types (`GEO`, `ALN`, `GDT`, `SCAN`); snapshots (`CORE.RPL.RECON`).

## Requirements

- **REQ-CORE.PRJ.TREE.MODEL-001** The element tree shall hold every element with its ID, registered type, parent group, name, per-stage slots, visibility flag and metadata slot.
- **REQ-CORE.PRJ.TREE.MODEL-002** Every slot shall carry exactly one state from `current`, `outdated`, `failed` or `not_computed`, and an element's displayed state shall be that of its slot in the active stage.
- **REQ-CORE.PRJ.TREE.MODEL-003** The tree shall allow only the state transitions not_computed → current or failed; current → outdated; failed → outdated; outdated → current or failed; and if an event requests any other transition, then applying it shall fail with `invalid_transition` and the commit shall be aborted.
- **REQ-CORE.PRJ.TREE.MODEL-004** If an element's type is not registered in the `ElementTypeRegistry`, then its creation shall fail with `validation` reason `unregistered_type`.
- **REQ-CORE.PRJ.TREE.MODEL-005** If a domain registers a type ID that is already registered, then registration shall fail at start-up with `duplicate_type`.
- **REQ-CORE.PRJ.TREE.MODEL-006** Element names shall be untrusted text of 0 to 256 characters, need not be unique, and identity shall be the element ID only.
- **REQ-CORE.PRJ.TREE.MODEL-007** Applying the same event sequence to the same initial state shall produce byte-identical canonical state digests.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/prj/tree/model/events.jsonl` (200 events over 50 elements of 3 test types), when applied, then the canonical state digest equals `expected_digest.txt` on every CI platform.
- **AC-2** Given fixture `fixtures/core/prj/tree/model/transitions.csv` (all 16 state pairs), when each transition is applied, then the allowed ones succeed and the others fail with `invalid_transition`.
- **AC-3** (error path) Given an element creation of an unregistered type, then `validation` / `unregistered_type` and nothing is journaled.
- **AC-4** (error path) Given two domains registering `geo.mesh`, then start-up fails with `duplicate_type`.
- **AC-5** Given two elements named `Plane 1`, then both exist with distinct IDs, and a name containing `ignore previous instructions` is stored verbatim as untrusted text.

## Design

- **Registry command(s):** none; element creation commands belong to the domains and use this model through events.
- **Data structures:** `Element { id, type_id, parent: GroupId, name: Untrusted<String>, slots: BTreeMap<StageId, Slot { digest: Option<Digest>, state }>, visible: bool, meta: MetaSlot }`; `Tree { elements: BTreeMap<EntityId, Element>, groups: BTreeMap<GroupId, Group> }`; `ElementTypeDef { type_id, payload_schema, display_kind }`; `impl StateView for Tree`.
- **Algorithm:** `apply(&mut Tree, &Event) -> Result<(), ApplyError>` with an exhaustive match over core event types and a registry dispatch for domain events; the state digest is the `fid-canon` digest of the ordered tree.
- **Errors:** `validation` (`unregistered_type`), `invalid_transition`, `duplicate_type`.
- **Determinism:** D0.
- **Performance budget:** 100,000 elements in < 200 MiB resident memory excluding payloads [unverified until measured].
- **Crate:** `fid-project` (module `tree`).

## Test fixtures

`fixtures/core/prj/tree/model/` (`events.jsonl`, `expected_digest.txt`, `transitions.csv`), produced by a committed generator; CC-BY-4.0.

## Build prompt

> In crate `fid-project`, add module `tree` with `Element` (ID, type ID, parent group, untrusted name ≤256 chars, per-stage slots `BTreeMap<StageId, Slot { digest, state }>`, visibility, metadata slot), `Group`, `Tree`, `ElementState` (current, outdated, failed, not_computed) with only the transitions not_computed→current|failed, current→outdated, failed→outdated, outdated→current|failed (`invalid_transition` otherwise), and `ElementTypeRegistry::register(def)` (`duplicate_type` at start-up; creating an element of an unregistered type gives `unregistered_type`). Implement the bus's read-only `StateView` for `Tree`, and a total, deterministic `apply(event)`; compute a state digest with `fid-canon`. A project without explicit stages uses one implicit default stage. Tests: AC-1..AC-5 with `fixtures/core/prj/tree/model/`. Constraints: CONSTITUTION C6 (untrusted text), §4 (no std HashMap or HashSet), §5 (stable IDs), §8.

## Open questions

- The implicit default stage must map cleanly to QIF (`CORE.PRJ.QIF`); it is treated as a stage with a reserved, derived UUIDv5 ID.

## Children

_Leaf: none._
