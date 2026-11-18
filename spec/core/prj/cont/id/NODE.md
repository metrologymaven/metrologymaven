---
id: CORE.PRJ.CONT.ID
title: Entity ID service
level: 4
parent: CORE.PRJ.CONT
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-project]
built_pr: 50
satisfies: [REQ-CORE.PRJ.CONT-003, REQ-CORE.PRJ.CONT-004, REQ-CORE.PRJ.CONT-005, REQ-CORE.PRJ.CONT-006]
covers: []
depends_on: [CORE.JRN.CANON]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.PRJ.CONT.ID: Entity ID service

## Purpose

Gives every entity a stable ID (CONSTITUTION §5): a fresh UUIDv7 for a new entity, a UUIDv5 for an entity derived deterministically from others, and the imported ID unchanged for an imported entity. The time and random bits of each UUIDv7 come from an observation source that records them, so replay substitutes the recorded values and reproduces the same IDs.

## Scope

**In:** `IdService::new_v7`, `derive_v5`, `import`; the project namespace UUID; the canonical name rule for UUIDv5; the `ObservationSource` trait (clock and CSPRNG, recorded); collision checks for imports.
**Out:** substituting recorded observations during replay (`CORE.RPL.EXEC`); container entities (`CORE.PRJ.CONT.HIER`); template-derived IDs, which use `derive_v5` (`CORE.PRJ.TPL.CREATE`).

## Requirements

- **REQ-CORE.PRJ.CONT.ID-001** When `new_v7` is called, the ID service shall build a UUIDv7 (RFC 9562) from a millisecond timestamp and 74 random bits drawn from the observation source, and the source shall record both values in the handler's observed inputs.
- **REQ-CORE.PRJ.CONT.ID-002** When the observation source is in replay mode, `new_v7` shall return the recorded ID in order and draw nothing new.
- **REQ-CORE.PRJ.CONT.ID-003** When `derive_v5` is called, the ID service shall return the UUIDv5 (RFC 9562) of the project namespace UUID and the `fid-canon` canonical bytes of the derivation inputs.
- **REQ-CORE.PRJ.CONT.ID-004** When `import` is called with a well-formed UUID or a QIF persistent ID mapped to one, the ID service shall return it unchanged, and if it already exists in the project, then return `id_collision`.
- **REQ-CORE.PRJ.CONT.ID-005** The ID service shall offer no operation that changes an existing entity's ID.

## Acceptance criteria

- **AC-1** Given a fake observation source with fixed time and random bits from fixture `fixtures/core/prj/cont/id/v7.json`, when `new_v7` runs, then each ID equals the expected value and has version 7 and variant 10.
- **AC-2** Given the recorded observations of AC-1 and replay mode, then the same IDs are returned and the CSPRNG is never called.
- **AC-3** Given fixture `fixtures/core/prj/cont/id/v5.json` (namespace, inputs, expected IDs from an independent reference script), then every `derive_v5` result matches.
- **AC-4** (error path) Given an import of an ID already in the project, then `id_collision`.

## Design

- **Registry command(s):** none; library used by handlers.
- **Data structures:** `IdService { namespace: Uuid, source: &dyn ObservationSource, existing: &dyn IdIndex }`; `trait ObservationSource { fn now_ms(&mut self) -> u64; fn random_bits(&mut self, n: u32) -> Bits; }` with live and replay implementations.
- **Algorithm:** RFC 9562 layouts for versions 5 and 7; SHA-1 for v5 as the RFC requires (identity only, not security).
- **Errors:** `id_collision`, `bad_id`.
- **Determinism:** D0 given the observation source; the live source is class N and its values are recorded.
- **Performance budget:** < 1 µs per ID [unverified until measured].
- **Crate:** `fid-project` (module `id`).

## Test fixtures

`fixtures/core/prj/cont/id/v7.json` and `v5.json`, produced by a committed reference script; CC-BY-4.0.

## Build prompt

> In crate `fid-project`, add module `id` with `IdService { new_v7, derive_v5, import }` and `trait ObservationSource { now_ms, random_bits }` (live: system clock and CSPRNG, recording each value; replay: returns recorded values in order). UUIDv7 and UUIDv5 per RFC 9562; v5 names are the `fid-canon` canonical bytes of the derivation inputs under the project namespace UUID. `import` keeps the given ID and fails with `id_collision` if it exists. No API changes an existing ID. Tests: AC-1..AC-4 with `fixtures/core/prj/cont/id/`. Constraints: CONSTITUTION §5 (stable IDs), §4 (replayable observed inputs), §8.

## Open questions

- None.

## Children

_Leaf: none._
