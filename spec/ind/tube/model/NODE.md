---
id: IND.TUBE.MODEL
title: Tube element and nominal
level: 3
parent: IND.TUBE
kind: feature
status: leaf
phase: P3
size: S
owners: []
claimed_by:
satisfies: [REQ-IND.TUBE-001]
covers: [PW.IND.04, PCD.IND.04]
depends_on: [IND.TUBE.BEND]
side_effect: W2
determinism: D1
cli: [fid pack create-tube]
mcp: [pack_create_tube]
journal: [pack.create_tube@1]
updated: 2026-11-16
---

# IND.TUBE.MODEL: Tube element and nominal

## Purpose

The `Tube` element: the nominal centreline as XYZ corner points, the outside diameter, the centreline radius of each bend, an optional wall thickness and an allowance at each end. It is validated through the bend-data conversion so a nominal that cannot be bent (collinear, reversed or overlapping bends) is refused at creation.

## Scope

**In:**
- Element type `Tube` registered through the tube pack manifest
- Command `pack.create_tube@1` (W2; agents write to an agent branch)
- Validation, including a call to the `IND.TUBE.BEND` conversion
- Storing the nominal LRA data alongside the XYZ points

**Out:**
- Importing nominals from bender files (owned by `IND.TUBE.EXP` if ever needed, or by IO)
- Measured reconstruction and comparison (owned by `IND.TUBE.RECON` and `IND.TUBE.CMP`)

## Requirements

- **REQ-IND.TUBE.MODEL-001** The `Tube` element type and `pack.create_tube@1` shall be registered only through the tube pack manifest.
- **REQ-IND.TUBE.MODEL-002** When `pack.create_tube@1` runs, the pack shall store the nominal XYZ points, outside diameter, one CLR per bend, optional wall thickness and the two end allowances, and shall journal one event.
- **REQ-IND.TUBE.MODEL-003** When a tube is created, the pack shall convert the nominal to LRA with `IND.TUBE.BEND` and store the LRA data with the element; any conversion error shall refuse the creation with the same error code and journal nothing.
- **REQ-IND.TUBE.MODEL-004** If the outside diameter is not positive and finite, a wall thickness is not below half the outside diameter, a CLR is below half the outside diameter, or an end allowance is negative, then the command shall fail with `E_TUBE_OD`, `E_TUBE_WALL`, `E_TUBE_CLR` or `E_TUBE_END` and journal nothing.
- **REQ-IND.TUBE.MODEL-005** When the command is invoked by an agent, the element shall be written to the agent's branch and provenance-tagged as agent-authored (CONSTITUTION C5).

## Acceptance criteria

- **AC-1** Given fixture `fixtures/pack/tube/model-3bend` (4 corner points with 3 bends, OD 0.012 m, CLR 0.03 m each, wall 0.001 m, end allowances 0.01 m), when `fid pack create-tube --json` runs, then a `Tube` element is created with those values and LRA data equal to `pack.convert_tube_benddata@1`'s output, and one journal event is written.
- **AC-2** (errors) Given OD 0, wall 0.006 m with OD 0.012 m, CLR 0.005 m with OD 0.012 m, or end allowance -0.001 m, when the command runs, then it fails with `E_TUBE_OD`, `E_TUBE_WALL`, `E_TUBE_CLR` or `E_TUBE_END` and the journal is unchanged.
- **AC-3** (bend errors) Given collinear corner points or a CLR whose setback exceeds a straight, when the command runs, then it fails with `E_TUBE_COLLINEAR` or `E_TUBE_BEND_OVERLAP` and the journal is unchanged.
- **AC-4** (agent) Given an MCP client, when it calls `pack_create_tube` with the AC-1 data, then the element lands on the agent branch with agent provenance and the working document is unchanged.
- **AC-5** (replay) Given the journal from AC-1, when replayed in exact mode on the same platform, then the outcome is `IDENTICAL` and the element ID is unchanged.

## Design

- **Registry command:** `pack.create_tube@1`, class W2. Parameters: `name: String`, `xyz: Vec<[f64;3]>` (m), `od: f64` (m), `clr: Vec<f64>` (m), `wall: Option<f64>` (m), `end_allowance: [f64; 2]` (m). Result: element ID (UUIDv7).
- **Data structures:** `Tube { id, name, xyz, od, clr, wall, end_allowance, lra: LraData }` (`LraData` from `IND.TUBE.BEND`).
- **Algorithm:** validate scalars; call the bend conversion library function (not the registry command, to avoid a nested journal entry); store; journal.
- **Errors:** `E_TUBE_OD`, `E_TUBE_WALL`, `E_TUBE_CLR`, `E_TUBE_END`, plus all `IND.TUBE.BEND` errors.
- **Determinism:** D1 (inherits the bend conversion).
- **Performance budget:** negligible.
- **Crate:** `fid-pack-tube`.

## Test fixtures

- `fixtures/pack/tube/model-3bend`: hand-written nominal in the test tree. Licence: project.

## Build prompt

> In the Rust crate `fid-pack-tube`, add element type `Tube { id, name, xyz, od, clr, wall: Option<f64>, end_allowance: [f64;2], lra: LraData }` and registry command `pack.create_tube@1` (side-effect class W2; CLI `fid pack create-tube [--json]`; MCP tool `pack_create_tube`; journal event `pack.create_tube@1`), both registered only through the pack manifest and extension-point profile.
>
> Validate: `od` positive and finite (`E_TUBE_OD`); `wall < od/2` if given (`E_TUBE_WALL`); every CLR >= od/2 (`E_TUBE_CLR`); end allowances >= 0 (`E_TUBE_END`). Then call the XYZ-to-LRA library function of the same crate (from `pack.convert_tube_benddata@1`) and propagate its errors (`E_TUBE_COLLINEAR`, `E_TUBE_REVERSAL`, `E_TUBE_BEND_OVERLAP`, `E_TUBE_DEGENERATE`, `E_TUBE_CLR`, `E_NAN`). Store the element with its LRA data; new IDs are UUIDv7. On any error journal nothing. When invoked by an agent, write to the agent branch with agent provenance.
>
> Tests: a 3-bend nominal creates an element whose LRA equals the conversion command's output and journals once; each validation error; bend errors propagate; MCP call lands on the agent branch; exact replay `IDENTICAL`.
>
> Constraints: KNP-1 (no FMA, no HashMap or HashSet, NaN is an error); no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; at most 1 000 bends.

## Open questions

- End conditions beyond a plain length allowance (flares, fittings) need a tube-shop expert.

## Children

_Leaf: none._
