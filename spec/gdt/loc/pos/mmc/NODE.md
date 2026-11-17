---
id: GDT.LOC.POS.MMC
title: Position at MMC and LMC through the material-condition engine
level: 4
parent: GDT.LOC.POS
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.LOC.POS-001, REQ-GDT.LOC.POS-002, REQ-GDT.LOC.POS-004]
covers: [GOM.GDT.08, PW.GDT.04]
depends_on: [GDT.LOC.POS.RFS, GDT.ZONE.MATL, GDT.LOC.LRES]
uses_artifacts: [UNC.ORS.GDT]
side_effect: R1
determinism: D3
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.LOC.POS.MMC: Position at MMC and LMC through the material-condition engine

## Purpose

Position of a feature of size at MMC or LMC: run the interpretation variant the resolver selected (axis interpretation with bonus, or surface interpretation against a virtual-condition boundary) through `GDT.ZONE.MATL`, and fill the position payload with stated tolerance, bonus, actual size and virtual-condition size. Which variant decides and how the boundary is formed are DMT content that the material-condition engine executes; this leaf only assembles the position-specific problem and payload.

## Scope

**In:** for the axis variant, the RFS position value from `GDT.LOC.POS.RFS` and the bonus from the engine; for the surface variant, the boundary clearance from the engine with the boundary placed at true position in the DRF; `LocPayload` fields: value, stated, bonus, actual size, virtual-condition size; `local_optimum_only` pass-through; variant name in the result.
**Out:** bonus arithmetic, boundaries and actual size (owned by `GDT.ZONE.MATL`, `GDT.DIM.SIZEASME`); dual-variant reporting (owned by `GDT.LOC.POS.CMD`); variant selection (owned by `GDT.RES.RESOLVE` and DMT rows).

## Requirements

- **REQ-GDT.LOC.POS.MMC-001** For the axis variant, the function shall report the RFS position value, the stated tolerance and the bonus from the material-condition engine as separate fields.
- **REQ-GDT.LOC.POS.MMC-002** For the surface variant, the function shall report the minimum boundary clearance from the material-condition engine with the virtual-condition boundary placed at true position in the DRF, and the virtual-condition size.
- **REQ-GDT.LOC.POS.MMC-003** The result shall name the evaluated variant and carry `local_optimum_only` if the actual-size fit or the engine carries it.
- **REQ-GDT.LOC.POS.MMC-004** Values shall match every ORS-GDT position/MMC case in the P1a subset within its stated tolerance, including cases where the axis and surface variants disagree.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/loc/pos_hole_mmc.json` (hole offset giving RFS value 0.100 mm, stated 0.080 mm at MMC) and an engine stub returning bonus 0.030 mm, when the axis variant runs, then value 0.100 mm, stated 0.080 mm and bonus 0.030 mm are separate fields.
- **AC-2** Given the surface variant and an engine stub returning clearance −0.004 mm and virtual-condition size 9.920 mm, then the payload carries both and names `surface`.
- **AC-3** Given an engine stub flagging `local_optimum_only`, then the result carries it.
- **AC-4** (ORS) Every P1a ORS-GDT position/MMC case passes when the engine and suite exist.

## Design

- **Registry command(s):** none (kernel function `position_material(derived, surface_points, nominal, drf, chain, matl) -> PosMaterialValue`).
- **Data structures:** `PosMaterialValue { variant, value_m?, clearance_m?, stated, bonus?, actual_size?, vc_size?, flags }`.
- **Algorithm:** variant from the resolution; axis: `position_rfs` then engine bonus; surface: engine boundary containment at true position.
- **Errors:** pass-through from RFS and the engine; `UNSUPPORTED{variant}`.
- **Determinism:** D3 (actual-size envelope fits).
- **Performance budget:** dominated by fits [unverified].
- **Crate:** `fid-gps` (module `loc::position::material`).

## Test fixtures

Analytic holes and pins with planted offsets and sizes; engine stubs (CC0); ORS-GDT suite from `UNC.ORS.GDT`.

## Build prompt

> In crate `fid-gps`, module `loc::position::material`, implement `position_material(...)`: for the resolver-selected `axis` variant call `position_rfs` and the material-condition engine for bonus, reporting value, stated and bonus separately; for the `surface` variant ask the engine for the minimum clearance to the virtual-condition boundary placed at true position in the DRF and report clearance and virtual-condition size; name the variant; propagate `local_optimum_only`. Do not decide which variant governs. Tests per acceptance criteria with engine stubs. Constraints: CONSTITUTION §8, KNP-1, no std `HashMap`/`HashSet`.

## Open questions

- Runs end to end once `GDT.ZONE.MATL` exists; tests use stubs until then.

## Children

_Leaf: none._
