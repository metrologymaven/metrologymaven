---
id: GDT.CHAR.IDENT
title: Stable characteristic identity
level: 3
parent: GDT.CHAR
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-gdt]
built_pr: 301
satisfies: [REQ-GDT.CHAR-009]
covers: []
depends_on: [CORE.PRJ.CONT.ID]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-19
---

# GDT.CHAR.IDENT: Stable characteristic identity

## Purpose

Gives every characteristic one identity that never changes from plan to result to export: the QIF QPId where QIF provides a slot, otherwise a Fiducial `fx:qpid` attribute. It is a small library used by the create, edit, evaluate and export paths, so identity rules live in one place.

## Scope

**In:** identity assignment on creation (preserve imported QPId; otherwise UUIDv7 per CONSTITUTION §5); carrying the identity through edits, re-evaluation and export; the `fx:qpid` attribute for slots QIF lacks; identity comparison helpers.
**Out:** the characteristic types (owned by `GDT.CHAR.MODEL`); QIF writing (owned by `IO.QIF`); characteristic designator (balloon) numbering (owned by `RPT.BALLOON`).

## Requirements

- **REQ-GDT.CHAR.IDENT-001** When a characteristic is imported with a QPId, the system shall keep that QPId unchanged as its identity.
- **REQ-GDT.CHAR.IDENT-002** When a characteristic is created without an imported QPId, the system shall assign a UUIDv7 identity exactly once.
- **REQ-GDT.CHAR.IDENT-003** The identity shall be unchanged by edits, re-evaluation, re-pointing of the defaults profile, and QIF export followed by re-import.
- **REQ-GDT.CHAR.IDENT-004** Where the QIF element has no QPId slot, the identity shall be written as the `fx:qpid` attribute through QIF `Attributes` and read back from it on import.
- **REQ-GDT.CHAR.IDENT-005** If an edit attempts to change a characteristic's identity, then the system shall reject it with error `identity_immutable`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/ident/imported_qpid.qif`, when the characteristic is imported, edited (tolerance changed) and exported, then the exported QPId equals the imported one byte for byte.
- **AC-2** Given a characteristic created with no QPId, when it is edited three times and exported and re-imported, then the identity is the same UUIDv7 each time and its version nibble is 7.
- **AC-3** Given fixture `fixtures/gdt/ident/no_qpid_slot.qif` (an element without a QPId slot), when exported, then an `fx:qpid` attribute is present and re-import yields the same identity.
- **AC-4** (error path) Given an edit payload with a different identity, when applied, then it fails with `identity_immutable` and the characteristic is unchanged.
- **AC-5** (determinism) Given the same inputs and the same injected clock and RNG seed, then identity assignment returns identical results on every platform.

## Design

- **Registry command(s):** none of its own; called by `gdt.create_characteristic@1`, `gdt.edit_characteristic@1` and the export path.
- **Data structures:** `CharId(Uuid)` newtype with `source: Imported | Assigned`; `QpidSlot = Native | FxAttribute`. As built (PR #301): `CharIdentity` in module `chars::qpid`, equal, ordered and hashed by UUID value only; its characteristic key is the lowercase UUID, and the verbatim imported text is kept for native-slot export only.
- **Algorithm:** on create, take the imported QPId if present, else draw a UUIDv7 from the injected clock and seeded counter RNG (no system time inside the library); on edit, compare and reject a change; on export, write native slot or `fx:qpid`. As built: every assignment goes through the `CORE.PRJ.CONT.ID` ID service (`import_uuid` for imported QPIds, `new_v7` for new ones) and is checked against the project ID index, with no redraw on collision; the seeded counter RNG is a test-only observation source, and replay uses the recorded observations. An `fx:qpid` value is accepted only in the exact lowercase form this leaf writes.
- **Errors:** `identity_immutable`, `malformed_qpid`, plus `id_collision` and the observation errors passed through from `CORE.PRJ.CONT.ID`.
- **Determinism:** D0 given injected clock and seed.
- **Performance budget:** negligible (< 1 µs per call) [unverified].
- **Crate:** `fid-gdt`.

## Test fixtures

Two small hand-written QIF 3.0 files (`imported_qpid.qif`, `no_qpid_slot.qif`) authored in the repo under CC0; no third-party data.

## Build prompt

> In crate `fid-gdt`, implement module `ident` providing `CharId` (newtype over UUID with `source: Imported | Assigned`), `assign_identity(imported: Option<Qpid>, clock: &dyn Clock, rng: &mut CounterRng) -> CharId`, `check_unchanged(old: &CharId, new: &CharId) -> Result<(), IdentError>`, and QIF mapping helpers that write and read the `fx:qpid` attribute through QIF `Attributes` when the element has no native QPId slot. Rules: keep imported QPIds unchanged; assign UUIDv7 exactly once otherwise; reject identity changes with `IdentError::IdentityImmutable`; reject malformed QPIds with `IdentError::MalformedQpid`. Tests: import-edit-export keeps the QPId; created IDs are UUIDv7 and survive three edits plus export/re-import; `fx:qpid` round-trips; identity change is rejected; same clock and seed gives identical IDs. Constraints: no `unwrap`, `expect`, `panic`, `unsafe`; no std `HashMap`/`HashSet`; no system time or OS RNG inside the library (inject them); fixtures are CC0 files under `fixtures/gdt/ident/`.

## Open questions

- The `fx:qpid` attribute name is not yet reserved in the versioned `fx` catalogue (see the Open questions of `CORE.PRJ.QIF`). Until it is, a third-party UUID written under that name in the exact lowercase form would be adopted as the identity.
- End-to-end coverage of AC-1 to AC-3 through `IO.QIF` and `GDT.CHAR.MODEL` is AC-7 of `GDT.CHAR.MODEL.CMDS`; this leaf tests them on the identity fields of one element.

## Children

_Leaf: none._
