---
id: SCAN.THK.MAT
title: Material thickness to the nominal opposite side
level: 3
parent: SCAN.THK
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-SCAN.THK-001, REQ-SCAN.THK-004, REQ-SCAN.THK-005, REQ-SCAN.THK-006, REQ-SCAN.THK-007]
covers: [GOM.SRF.08, PW.CMP.05]
depends_on: [SCAN.THK.TFIELD, SCAN.CMP.DEV]
side_effect: W2
determinism: D0
cli: [fid compare thickness-material]
mcp: [compare_thickness_material]
journal: [compare.thickness_material@1]
updated: 2026-11-16
---

# SCAN.THK.MAT: Material thickness to the nominal opposite side

## Purpose

Material thickness when only one side of a sheet part was scanned: the distance from each measured point on side A to the nominal CAD surface of the opposite side B, in a stated alignment. It is computed with SCAN.CMP's deviation primitive, so it shares the sign conventions, search limits and `NO_MATCH` handling of surface comparison.

## Scope

**In:**
- `compare.thickness_material@1`: measured points (cloud or mesh) of one side against a selection of nominal faces forming the opposite side
- Thickness = negated signed deviation of the measured point from the opposite-side faces (the point lies on the material-inside side of those faces)
- Maximum thickness and angle cutoff; `NO_MATCH` handling
- Alignment recorded with the field

**Out** (and which node owns it instead):
- The deviation primitive (owned by `SCAN.CMP.DEV`)
- Wall thickness from two measured sides (owned by `SCAN.THK.RAY` and `SCAN.THK.SPHERE`)
- Field element, limits and statistics (owned by `SCAN.THK.TFIELD`)

## Requirements

- **REQ-SCAN.THK.MAT-001** When `compare.thickness_material@1` runs, the system shall create a `ThicknessField` through `ThicknessFieldBuilder` recording the measured source, the opposite-face selection, the alignment ID and all their digests, and shall modify no source.
- **REQ-SCAN.THK.MAT-002** For each measured point, the thickness shall be −d, where d is the `SCAN.CMP.DEV` signed deviation (method `shortest` or `normal` as stated) of the point against the opposite-side faces in the stated alignment.
- **REQ-SCAN.THK.MAT-003** If the primitive returns `NO_MATCH`, or −d is negative or exceeds the stated maximum thickness, then the point shall be `NO_MATCH` with no value.
- **REQ-SCAN.THK.MAT-004** The method shall be determinism class D0 and shall record definition ID `thk.material.v1` with the field.
- **REQ-SCAN.THK.MAT-005** On the stepped-plate fixture scanned from the top only, the method shall report the known thicknesses within 1 µm plus the scan's simulated noise standard deviation [unverified tolerance].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/thk_step_plate_onesided` (top-side cloud of the stepped plate with 5 µm noise, CAD, alignment, seed 67), when `fid compare thickness-material --source top --opposite-faces bottom --alignment a1 --method normal --max-thickness 0.01 --json` runs, then the region means are 2.000, 3.000 and 4.000 mm within 6 µm.
- **AC-2** Given the top cloud shifted +0.05 mm along +z before comparison (simulating thicker material), means increase by 0.050 mm ± 6 µm.
- **AC-3** Given an opposite-face selection that excludes the 4 mm region's bottom face, those points are `NO_MATCH`.
- **AC-4** (error path) Given an alignment in a different frame chain, the command fails with `geo.frame.mismatch` and journals nothing.
- **AC-5** (replay) Exact replay of AC-1 yields `IDENTICAL`.

## Design

- **Registry command:** `compare.thickness_material@1`, W2, D0. Parameters: `source` (cloud or mesh), `opposite_faces` (GEO selection on the CAD body), `alignment`, `method` (`normal` | `shortest`), `max_distance` = `max_thickness` m, `max_angle` rad (default 0.35), `selection?` on the source.
- **Algorithm:** call the DEV batch primitive with the opposite-face nominal; map each `VALID` d to −d; apply REQ-003.
- **Errors:** `geo.frame.mismatch`, `geo.selection.stale`, `scan.thk.bad_parameter`.
- **Determinism:** D0.
- **Performance budget:** as surface comparison of the same size (SCAN.CMP.DEV) [unverified].
- **Crate:** `fid-scan-thk`.

## Test fixtures

- `fixtures/scan/thk_step_plate_onesided`: generated top-side cloud (seed 67), CAD and alignment, CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-scan-thk`, implement the W2, D0 registry command `compare.thickness_material@1` (CLI `fid compare thickness-material --source <h> --opposite-faces <h> --alignment <h> [--method normal|shortest] --max-thickness M [--max-angle RAD] [--json]`, MCP `compare_thickness_material`). For each measured point compute d with the SCAN.CMP deviation primitive (`fid_scan_cmp::dev::deviation_batch`) against the selected opposite-side CAD faces in the stated alignment; thickness = −d; NO_MATCH when the primitive reports NO_MATCH or −d < 0 or −d > max thickness. Write results through `ThicknessFieldBuilder` with definition ID `thk.material.v1`, recording source, face selection, alignment ID and digests.
>
> Constraints: CPU only, KNP-1, no panic/unwrap/unsafe, sources never modified. Tests: one-sided stepped plate means within 6 µm, offset shift, excluded faces NO_MATCH, frame mismatch error, exact replay IDENTICAL.

## Open questions

- None.

## Children

_Leaf: none._
