---
id: GDT.CHAR.MODEL.CMDS
title: Create and edit characteristic commands
level: 4
parent: GDT.CHAR.MODEL
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.CHAR.MODEL-002, REQ-GDT.CHAR.MODEL-005]
covers: [PCD.DIM.02]
depends_on: [GDT.CHAR.MODEL.TYPES, CORE.PRJ, CORE.JRN]
side_effect: [W2, W2]
determinism: D0
cli: [fid gdt create-characteristic, fid gdt edit-characteristic]
mcp: [gdt_create_characteristic, gdt_edit_characteristic]
journal: [gdt.create_characteristic@1, gdt.edit_characteristic@1]
updated: 2026-11-17
---

# GDT.CHAR.MODEL.CMDS: Create and edit characteristic commands

## Purpose

The two registry commands that add a characteristic (with its feature control frame) to a plan and change one. Each commit journals the full post-change characteristic in canonical JSON, so replay and diff never need the previous state. Identity minting and governing-spec inheritance are injected as two small traits (`IdentityProvider`, `SpecInheritor`) that `GDT.CHAR.IDENT` and `GDT.CHAR.SPEC` implement and the registry wiring passes in, because both of those leaves already depend on this module and a direct call back would close a dependency cycle.

## Scope

**In:** `gdt.create_characteristic@1` and `gdt.edit_characteristic@1` (W2; agents write to an agent branch per CONSTITUTION §3); dry-run; validation through the type constructors; plan membership through `CORE.PRJ`; the journal payload.
**Out:** governing-spec setting and inheritance rules (owned by `GDT.CHAR.SPEC`); creating characteristics from PMI (owned by `GDT.PMI`); comparison-point creation (owned by `SCAN.CMP.CPT`, which calls `create_characteristic` with an `Extension` kind); datum definitions (owned by `GDT.DRF.DATUM`).

## Requirements

- **REQ-GDT.CHAR.MODEL.CMDS-001** When `gdt.create_characteristic@1` succeeds, the system shall add the characteristic to the named plan, assign identity through the injected `IdentityProvider`, apply governing-spec inheritance through the injected `SpecInheritor`, and journal one event whose payload is the full characteristic in RFC 8785 canonical JSON.
- **REQ-GDT.CHAR.MODEL.CMDS-002** When `gdt.edit_characteristic@1` succeeds, the system shall replace the named fields, keep the characteristic's identity unchanged, and journal one event whose payload is the full post-edit characteristic.
- **REQ-GDT.CHAR.MODEL.CMDS-003** The commands shall accept every FCF element the types represent (all modifiers, zone shapes, datum-reference modifiers, composite segments, simultaneous groups) as input.
- **REQ-GDT.CHAR.MODEL.CMDS-004** If validation fails or the plan or characteristic does not exist, then the command shall fail with the typed error and journal nothing.
- **REQ-GDT.CHAR.MODEL.CMDS-005** When run with `--dry-run`, the commands shall return the would-be characteristic and journal nothing.

## Acceptance criteria

- **AC-1** Given an empty plan from fixture `fixtures/gdt/model/plan_empty.json`, when `fid gdt create-characteristic --plan P --json` runs with input `fixtures/gdt/model/position_mmc_input.json`, then the plan has one characteristic, its id is the one returned by the deterministic test `IdentityProvider`, and exactly one journal event holds the canonical JSON equal to `expected/position_mmc_created.json`.
- **AC-2** Given AC-1, when `fid gdt edit-characteristic --char C --set tolerance=0.10mm --json` runs, then the id is unchanged and the journal payload equals `expected/position_mmc_edited.json`.
- **AC-3** (error path) Given a composite FCF over the QIF segment limit, then the command fails with `invalid_fcf` and journals nothing.
- **AC-4** (error path) Given an unknown characteristic id for edit, then `not_found` and nothing journaled.
- **AC-5** Given `--dry-run`, then the output equals AC-1's characteristic and the journal is unchanged.
- **AC-6** (replay) Exact replay of AC-1 and AC-2 gives `IDENTICAL`.

## Design

- **Registry command(s):** `gdt.create_characteristic@1` (W2; params `plan: PlanId`, `characteristic: CharacteristicInput`; result `Characteristic`); `gdt.edit_characteristic@1` (W2; params `char: CharId`, `set: FieldPatch`; result `Characteristic`). Both `--json` and `--dry-run`.
- **Data structures:** `CharacteristicInput` (Characteristic without identity); `FieldPatch` as a typed list of field paths and values (no free-form JSON Patch); traits `IdentityProvider { fn assign(&mut self, imported: Option<Qpid>) -> CharId; fn check_unchanged(..) }` and `SpecInheritor { fn inherit_on_create(&self, plan, &mut Characteristic) -> Result<InheritRecord, SpecError> }`, implemented in `GDT.CHAR.IDENT` and `GDT.CHAR.SPEC` and wired at registry construction.
- **Algorithm:** validate through constructors, mint or keep identity, apply inheritance, write to the plan through the CORE.PRJ API, journal.
- **Errors:** `invalid_fcf{reason}`, `not_found`, `plan_not_found`, `unknown_field`, `identity_immutable`, plus pass-through `SpecError` from the inheritor.
- **Determinism:** D0.
- **Performance budget:** < 2 ms per command excluding journal I/O [unverified].
- **Crate:** `fid-gdt` (module `cmd::characteristic`).

## Test fixtures

Synthetic plan and characteristic JSON (CC0, in-repo).

## Build prompt

> In crate `fid-gdt`, implement registry commands `gdt.create_characteristic@1` and `gdt.edit_characteristic@1` (both W2, `--json`, `--dry-run`; CLI `fid gdt create-characteristic` / `edit-characteristic`; MCP `gdt_create_characteristic` / `gdt_edit_characteristic`). Create validates input through the `model` type constructors, mints identity through an injected `IdentityProvider` trait and applies governing-spec inheritance through an injected `SpecInheritor` trait (both defined in this module; the identity and governing-spec modules implement them and registry wiring passes them in; tests use deterministic stubs), adds the characteristic to the plan through the CORE.PRJ API and journals one event with the full characteristic in RFC 8785 canonical JSON. Edit applies a typed field patch, keeps identity and journals the full post-edit characteristic. Errors journal nothing; dry-run journals nothing. Tests per acceptance criteria, including exact replay `IDENTICAL`. Constraints: CONSTITUTION §3 (agents write to an agent branch), §8 (no `unwrap`/`expect`/`panic`/`unsafe`), no std `HashMap`/`HashSet`.

## Open questions

- `GDT.CHAR.IDENT` and `GDT.CHAR.SPEC` declare `depends_on: [GDT.CHAR.MODEL]` (the whole module) although they need only `GDT.CHAR.MODEL.TYPES`. Narrowing them would let this leaf call them directly instead of through injected traits; that is their owners' change.

## Children

_Leaf: none._
