---
id: GEO.FRAME.CMD
title: Frame, transform and unit commands
level: 3
parent: GEO.FRAME
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-GEO.FRAME-003, REQ-GEO.FRAME-007, REQ-GEO.FRAME-008]
covers: []
depends_on: [GEO.FRAME.TREE, GEO.FRAME.APPLY, GEO.FRAME.UNITS, GEO.TYPES.REG, CORE.REG, CORE.JRN]
side_effect: W2
determinism: D0
cli: [fid geo create-frame, fid geo set-transform, fid geo delete-frame, fid geo apply-transform, fid geo resolve-transform, fid geo convert-units]
mcp: [geo_create_frame, geo_set_transform, geo_delete_frame, geo_apply_transform, geo_resolve_transform, geo_convert_units]
journal: [geo.create_frame@1, geo.set_transform@1, geo.delete_frame@1, geo.apply_transform@1, geo.resolve_transform@1, geo.convert_units@1]
updated: 2026-11-16
---

# GEO.FRAME.CMD: Frame, transform and unit commands

## Purpose

The registry commands through which people, scripts and agents change the frame tree, move elements between frames and convert units, so that every frame relationship change is one journaled W2 event and every authored length carries its conversion record.

## Scope

**In:**
- `geo.create_frame@1` (W2), `geo.set_transform@1` (W2, also reparents), `geo.delete_frame@1` (W2)
- `geo.apply_transform@1` (W2): new element in the target frame, registered as derived
- `geo.resolve_transform@1` (R1) and `geo.convert_units@1` (R1)
- Transform parameters accepted as matrix, quaternion, axis-angle or Euler, with translations as authored quantities
- Refusing to delete a frame that any element still references

**Out:**
- Computing alignment transforms (owned by `ALN`)
- Tree, transform and unit logic (owned by `GEO.FRAME.TREE`, `GEO.FRAME.XFORM`, `GEO.FRAME.UNITS`)

## Requirements

- **REQ-GEO.FRAME.CMD-001** Each of `geo.create_frame@1`, `geo.set_transform@1`, `geo.delete_frame@1` and `geo.apply_transform@1` shall be registered as class W2 and shall produce exactly one Record Journal entry when it commits.
- **REQ-GEO.FRAME.CMD-002** When a W2 frame command receives a length as an authored quantity, its journal entry arguments shall include the conversion record `{ text, unit, si_hex }`.
- **REQ-GEO.FRAME.CMD-003** If `geo.delete_frame@1` targets a frame referenced by any element or child frame, then it shall fail with `frame_in_use` listing up to 20 referencing IDs and journal nothing.
- **REQ-GEO.FRAME.CMD-004** When `geo.apply_transform@1` commits, the new element shall be registered as derived from the source element and the frames, so CORE.DEP recomputes it when either frame's transform changes.
- **REQ-GEO.FRAME.CMD-005** `geo.resolve_transform@1` and `geo.convert_units@1` shall be class R1, write no Record Journal entry and return the result as JSON with matrices in decimal and IEEE hex.
- **REQ-GEO.FRAME.CMD-006** When an agent principal invokes a W2 frame command over MCP, the command shall commit to the agent's branch per CORE.BUS rules.

## Acceptance criteria

- **AC-1** Given an empty project, when `fid geo create-frame --name part_a --parent world --translation "10 mm,0 mm,5 mm" --rotation-euler ZYX:0,0,90deg --json` runs, then the frame exists, the Record Journal grows by one `geo.create_frame@1` entry, and its arguments include three conversion records with `si_hex` for 0.01, 0 and 0.005.
- **AC-2** Given fixture `fixtures/geo/types/pointset_small` in world, when `fid geo apply-transform <id> --to part_a --json` runs, then a new element exists in `part_a`, coordinates match the GEO.FRAME.APPLY result bit-for-bit, and a later `geo.set_transform@1` on `part_a` marks it `outdated`.
- **AC-3** (error path) Given a frame referenced by an element, when `fid geo delete-frame` runs, then it fails with `frame_in_use` and the journal length is unchanged.
- **AC-4** Given `tree_7` loaded, when `fid geo resolve-transform --from align_1 --to inst_2 --json` runs, then the matrix equals the fixture value and no Record Journal entry is written.
- **AC-5** (replay) Given the journal from AC-1 and AC-2, when replayed in exact mode, then the outcome is `IDENTICAL`.
- **AC-6** Given the generated manifest, then the six MCP tools exist with the names in this node, W2 tools are projected as named tools and R1 tools as exposed.

## Design

- **Registry command(s):**
  - `geo.create_frame@1` W2: `{ name, kind, parent: FrameId, rotation: RotationParam, translation: [AuthoredLength; 3], scale?: decimal }` → `FrameId`.
  - `geo.set_transform@1` W2: `{ frame, parent?, rotation, translation, scale? }` → tree digest.
  - `geo.delete_frame@1` W2: `{ frame }` → tree digest.
  - `geo.apply_transform@1` W2: `{ element, to: FrameId }` → new element ID.
  - `geo.resolve_transform@1` R1: `{ from, to }` → `{ matrix, matrix_hex, scale }`.
  - `geo.convert_units@1` R1: `{ text, unit, to_unit?, digits? }` → conversion record and optional display string.
- **Data structures:** `RotationParam` (tagged union of matrix, quaternion, axis-angle, Euler); `AuthoredLength` (text plus unit).
- **Algorithm:** each command validates, calls the library leaf, registers results via GEO.TYPES.REG and returns; no geometry logic lives here.
- **Errors:** `frame_in_use`, `unknown_frame`, `cycle`, `not_orthonormal`, `bad_quantity`, `frame_mismatch`.
- **Determinism:** D0 (library operations are D0).
- **Performance budget:** command overhead under 2 ms excluding element transformation [unverified].
- **Crate:** `fid-geo-cmd`.

## Test fixtures

- `tree_7.json`, `pointset_small`, and an in-memory test project from CORE's harness.

## Build prompt

> In crate `fid-geo-cmd`, register six commands with the CORE registry (CONSTITUTION §3 naming; CLI, MCP tools and docs are generated from the manifest, never hand-written): `geo.create_frame@1` (W2), `geo.set_transform@1` (W2, may reparent), `geo.delete_frame@1` (W2), `geo.apply_transform@1` (W2), `geo.resolve_transform@1` (R1), `geo.convert_units@1` (R1). Parameters as in the Design section; rotations accept matrix, quaternion, axis-angle or Euler; lengths are authored quantities parsed by `fid-geo-frame::units`, and W2 entries include the `{text, unit, si_hex}` record. Delete fails with `frame_in_use` if any element or child frame references the frame. Apply registers the new element as derived (UUIDv5) with dependency edges to the source element and both frames. Agents' W2 calls go to their branch per CORE.BUS. Acceptance: AC-1..AC-6 (create with three conversion records; apply bit-exact and outdated on frame change; delete refused with no journal growth; resolve R1 with no Record Journal entry; exact replay `IDENTICAL`; six tools in the manifest with correct projections). Constraints: CONSTITUTION §3, §4 (single journaled rounding), §8 (no panic, no `unsafe`). Files: `crates/fid-geo-cmd/src/frame_cmds.rs`, `tests/frame_cmds.rs`.

## Open questions

- None.

## Children

_Leaf: none._
