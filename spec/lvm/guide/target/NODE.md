---
id: LVM.GUIDE.TARGET
title: Build targets and tolerance bands
level: 3
parent: LVM.GUIDE
kind: feature
status: leaf
phase: P3
size: M
owners: []
claimed_by:
satisfies: [REQ-LVM.GUIDE-001]
covers: []
depends_on: [LVM.STATION.MODEL, GEO.FRAME]
side_effect: W2
determinism: D0
cli: [fid build create-target, fid build remove-target, fid build list-targets]
mcp: [build_create_target, build_remove_target, build_list_targets]
journal: [build.create_target@1, build.remove_target@1]
updated: 2026-11-16
---

# LVM.GUIDE.TARGET: Build targets and tolerance bands

## Purpose

Defines what an operator is setting: a build target is a nominal location, either a point or a point on a nominal surface, expressed in a named frame, with one or more tolerance bands. Each band is either along a frame axis or along a stated direction (for a surface target, its normal). Guidance (`LVM.GUIDE.DIRECT`), alarms and sessions all read these definitions.

## Scope

**In:**
- `BuildTarget`: ID, label, kind (`point` or `surface`), frame reference, nominal point, optional surface reference and normal, tolerance bands, optional sequence index
- Tolerance bands: per axis (x, y, z) or along a unit direction, each with lower and upper limits
- Validation, removal (kept in the journal) and listing

**Out** (and which node owns it instead):
- Deviation and move-direction computation (`LVM.GUIDE.DIRECT`)
- Sequences and set-in (`LVM.GUIDE.SESSION`)
- Alarm rules (`LVM.GUIDE.ALARM`)
- Surface geometry and frames (`GEO.CAD`, `GEO.FRAME`)

## Requirements

- **REQ-LVM.GUIDE.TARGET-001** When `build.create_target@1` runs, the system shall store a build target with a UUIDv7 ID, unique label, kind, frame reference, nominal point in that frame, and at least one tolerance band.
- **REQ-LVM.GUIDE.TARGET-002** Each tolerance band shall be either per axis (x, y or z of the target frame) or along a direction given as a vector, which the system shall normalise to unit length with the normalisation journaled.
- **REQ-LVM.GUIDE.TARGET-003** If a band has lower ≥ upper, a band does not contain zero (lower > 0 or upper < 0), a direction has length below 1e-12, any value is non-finite, or no band is given, then the command shall fail with `BUILD_TARGET_INVALID` naming the field and journal nothing.
- **REQ-LVM.GUIDE.TARGET-004** When the kind is `surface`, the system shall store the surface reference and the unit normal at the nominal point, and shall require a band along that normal.
- **REQ-LVM.GUIDE.TARGET-005** Tolerance limits shall be stored in metres with their authored values and units kept, each conversion being one journaled rounding.
- **REQ-LVM.GUIDE.TARGET-006** When `build.remove_target@1` runs, the system shall mark the target removed and hide it from `build.list_targets`, keeping it in the document so undo and replay restore it.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/lvm/build-targets` (a frame `JIG` and 5 target specs: 3 point targets with per-axis bands, 1 point target with a direction band, 1 surface target), when each is created with `fid build create-target ... --json`, then `fid build list-targets --json` shows 5 targets with limits in metres, authored strings kept and direction bands of unit length within 1e-15.
- **AC-2** (error path) Given a band with lower = +0.1 mm and upper = +0.3 mm, when the target is created, then the command fails with `BUILD_TARGET_INVALID` naming `band[0]` and the journal length is unchanged.
- **AC-3** (error path) Given a surface target without a band along its normal, then the command fails with `BUILD_TARGET_INVALID` naming `normal_band`.
- **AC-4** (remove) Given one target removed, then it is not listed; after journal undo it is listed again with identical fields.
- **AC-5** (replay) Given the journal from AC-1 to AC-4, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry commands:** `build.create_target@1` (W2): `label, kind: Point|Surface, frame: FrameRef, nominal: [Quantity; 3], surface: Option<SurfaceRef>, normal: Option<[f64; 3]>, bands: Vec<BandSpec>, sequence_index: Option<u32>` → `TargetId`; `build.remove_target@1` (W2); `build.list_targets@1` (R0).
- **Data structures:** `BuildTarget { id, label, kind, frame, nominal_m: [f64; 3], surface, normal, bands: Vec<Band>, sequence_index, removed: bool }`; `Band { axis: X|Y|Z|Dir([f64; 3]), lower_m, upper_m, authored }`.
- **Normalisation:** divide by the norm computed with fixed-order sum and correctly rounded sqrt; no FMA.
- **Errors:** `BUILD_TARGET_INVALID { field }`, `NET_DUPLICATE_LABEL`, `BUILD_UNKNOWN_TARGET`.
- **Determinism:** D0.
- **Crate:** `fid-lvm` (proposed name; build-guidance module).

## Test fixtures

- `fixtures/lvm/build-targets`: hand-written JSON target specs and a frame definition; project-authored, CC-BY-4.0.

## Build prompt

> In the crate `fid-lvm`, add build targets for fixture and build guidance. Register `build.create_target@1` (W2), `build.remove_target@1` (W2) and `build.list_targets@1` (R0), with CLI `fid build create-target|remove-target|list-targets` and `--json`; MCP names are generated. A target has a UUIDv7 ID, unique label, kind `point` or `surface`, a frame reference, a nominal point in that frame, an optional surface reference with unit normal, an optional sequence index and at least one tolerance band; a band is along x, y or z of the frame or along a direction vector (normalised, with the normalisation journaled) and has lower < upper with lower ≤ 0 ≤ upper. Surface targets require a band along their normal. Fail with `BUILD_TARGET_INVALID { field }` and journal nothing on any violation or non-finite value. Limits are stored in metres with authored text kept; conversions are one journaled rounding each (CONSTITUTION §4). Removal marks the target removed; undo restores it. No FMA, no std HashMap, no panics, no `unwrap`, no `unsafe` (CONSTITUTION §4, §8). Tests: the five acceptance criteria above. Determinism class D0.

## Open questions

- Should asymmetric tolerance zones other than bands (for example a cylindrical position zone) be offered? Not in this leaf.

## Children

_Leaf: none._
