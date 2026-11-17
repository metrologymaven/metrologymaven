---
id: CMM.COLL.MACH
title: Machine model, envelope and obstruction bodies
level: 3
parent: CMM.COLL
kind: feature
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-CMM.COLL-001, REQ-CMM.COLL-002]
covers: [PCD.PTH.06]
depends_on: [GEO]
side_effect: W2
determinism: D0
cli: [fid cmm define-machine, fid cmm show-machine]
mcp: [cmm_define_machine, cmm_show_machine]
journal: [cmm.define_machine@1]
updated: 2026-11-16
---

# CMM.COLL.MACH: Machine model, envelope and obstruction bodies

## Purpose

Defines the machine the probe moves in, for a 3-axis Cartesian CMM: the travel envelope (seeded from controller-reported limits when available), an envelope margin, and obstruction bodies (table, columns, bridge, ram) attached either to the machine base or to a moving axis. Its digest is the run ticket's `machine_model`.

## Scope

**In:**
- `MachineModel` entity: ID, version, digest, axis order and directions, envelope per axis, margin, obstruction bodies with their attachment (`BASE`, `X`, `Y`, `Z`)
- Bodies as boxes, cylinders or mesh references (by GEO mesh digest)
- Seeding the envelope from controller-reported limits supplied as data, and rejecting a user envelope beyond them
- Body poses for a given axis position (3-axis Cartesian kinematics)
- `cmm.define_machine@1` (W2) and `cmm.show_machine@1` (R0)

**Out** (and which node owns it instead):
- Reading limits from the controller over I++ (owned by `HAL`; this leaf receives them as a typed input)
- Rotary tables (owned by `CMM.COLL.ROT`)
- Collision queries (owned by `CMM.COLL.CHECK`)
- Mesh storage (owned by `GEO`)

## Requirements

- **REQ-CMM.COLL.MACH-001** The system shall represent a machine model with a stable ID, a version, a per-axis envelope (minimum and maximum in metres), an envelope margin and a list of obstruction bodies, and shall compute its digest as SHA-256 over RFC 8785 canonical JSON of all fields except the display name.
- **REQ-CMM.COLL.MACH-002** When controller-reported travel limits are supplied, the system shall seed any unspecified envelope bound from them and shall reject a user-entered bound that lies outside them with `MACHINE_ENVELOPE_EXCEEDS_CONTROLLER` naming the axis.
- **REQ-CMM.COLL.MACH-003** If an envelope has minimum ≥ maximum on any axis, a negative margin, or a margin of at least half the smallest axis range, then the system shall reject the model with `MACHINE_ENVELOPE_INVALID`.
- **REQ-CMM.COLL.MACH-004** The system shall attach each obstruction body to `BASE`, `X`, `Y` or `Z`, and, for a given axis position, shall return each body's pose by composing the attachment chain in the declared axis order.
- **REQ-CMM.COLL.MACH-005** The system shall record whether the envelope was seeded from controller limits, user-entered, or both, together with the controller identity that supplied the limits.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/cmm/coll/bridge_cmm.json` (bridge CMM 1.0 × 0.7 × 0.6 m with table, two columns, bridge and ram) and `controller_limits.json`, when `fid cmm define-machine --file bridge_cmm.json --controller-limits controller_limits.json --json` runs, then the model is created with the fixture's `expected_digest` and `envelope_source: CONTROLLER_AND_USER`.
- **AC-2** Given the AC-1 model, when `fid cmm show-machine --id <id> --axis-position 0.5,0.3,-0.2 --json` runs, then the ram body pose equals `bridge_cmm_expected_poses.json` bit for bit.
- **AC-3** (error path) Given a user Y maximum of 0.8 m and controller Y maximum of 0.7 m, when defined, then it fails with `MACHINE_ENVELOPE_EXCEEDS_CONTROLLER` naming `Y` and journals nothing.
- **AC-4** (error path) Given a margin of 0.4 m on a 0.6 m Z range, when defined, then it fails with `MACHINE_ENVELOPE_INVALID`.
- **AC-5** Given AC-1 with one column moved by 1 mm, when defined as an edit, then a new version with a different digest is created.
- **AC-6** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry commands:** `cmm.define_machine@1` (W2; params `MachineModelInput`, `controller_limits: Option<ControllerLimits>`, `edit_of`; result `{machine_id, version, digest, envelope_source}`); `cmm.show_machine@1` (R0; params `machine_id`, `version`, `axis_position: Option<[f64; 3]>`; result model plus body poses).
- **Data structures:** `MachineModel { id, version, name: UntrustedText, axes: [AxisDef; 3], envelope: [(f64, f64); 3], margin_m, bodies: Vec<Body { id, shape: Box | Cylinder | MeshRef(Digest), attach: BASE | X | Y | Z, pose }>, envelope_source, controller_identity: Option<String>, digest }`; `ControllerLimits { controller_identity, min: [f64; 3], max: [f64; 3] }`.
- **Algorithm:** translation-only kinematics for 3-axis Cartesian machines: body pose = base pose ∘ translation of each axis up to and including its attachment, in declared axis order.
- **Errors:** `MACHINE_ENVELOPE_INVALID`, `MACHINE_ENVELOPE_EXCEEDS_CONTROLLER { axis }`, `MACHINE_BODY_INVALID`, `MACHINE_NOT_FOUND`.
- **Determinism:** D0.
- **Crate:** `fid-cmm-coll`.

## Test fixtures

Synthetic machine in `fixtures/cmm/coll/` (`bridge_cmm.json`, `controller_limits.json`, `bridge_cmm_expected_poses.json`), with generic dimensions not taken from any vendor drawing, CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-cmm-coll`, implement the 3-axis Cartesian machine model with commands `cmm.define_machine@1` (W2, journaled) and `cmm.show_machine@1` (R0); CLI `fid cmm define-machine`, `fid cmm show-machine` with `--json`; MCP generated.
> The model has per-axis envelope (min < max, metres), a margin (0 ≤ margin < half the smallest range; else `MACHINE_ENVELOPE_INVALID`), and obstruction bodies (box, cylinder or mesh digest) attached to BASE, X, Y or Z. Controller-reported limits (typed input) seed unspecified bounds; a user bound outside them fails with `MACHINE_ENVELOPE_EXCEEDS_CONTROLLER {axis}`. Record the envelope source and controller identity. Body poses at an axis position are translation compositions in declared axis order. Digest = SHA-256 over RFC 8785 JCS of all fields but the name; edits create new versions.
> Acceptance tests: the six criteria in spec node `CMM.COLL.MACH`, fixtures in `fixtures/cmm/coll/`.
> Constraints: no FMA; NaN is an error; no panic, no `unsafe`, no `unwrap`/`expect`; determinism D0.

## Open questions

- Horizontal-arm and gantry machines use the same translation-only model with different axis orders; non-Cartesian machines are out of scope.

## Children

_Leaf: none._
