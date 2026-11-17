---
id: SCAN.CMP.DEV.SURFCAD
title: Surface comparison against CAD command
level: 4
parent: SCAN.CMP.DEV
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.CMP.DEV-003, REQ-SCAN.CMP.DEV-004, REQ-SCAN.CMP.DEV-006, REQ-SCAN.CMP.DEV-007]
covers: [GOM.SRF.01, PW.CMP.01, SA.CAD.02, SA.SCN.02, PCD.LAS.02]
depends_on: [SCAN.CMP.DEV.PRIM, SCAN.CMP.DEV.FIELD, SCAN.MESHOP.PROV, GEO.OOC.STREAM, GEO.SEL.MODEL, GEO.FRAME.APPLY, ALN.MODEL.ELEM]
side_effect: W2
determinism: D0
cli: [fid compare surface-cad]
mcp: [compare_surface_cad]
journal: [compare.surface_cad@1]
updated: 2026-11-17
---

# SCAN.CMP.DEV.SURFCAD: Surface comparison against CAD command

## Purpose

The command behind every scan-to-CAD colour map: it compares a cloud or mesh with CAD faces or a nominal mesh, in a named alignment, optionally restricted to selections, streaming out-of-core data, and stores a `DeviationField`. Mesh actuals have vertices backed only by synthesized triangles excluded.

## Scope

**In:**
- `compare.surface_cad@1` with actual (cloud, out-of-core cloud or mesh), nominal (CAD body, CAD face selection or nominal mesh; kind inferred), method, search distance, angle cutoff, alignment, actual and nominal selections
- Streaming of actual points in original index order through GEO.OOC
- `EXCLUDED` status for mesh vertices used only by synthesized triangles
- The performance budget at 10⁸ points

**Out** (and which node owns it instead):
- The primitive (owned by `SCAN.CMP.DEV.PRIM`); the field type (owned by `SCAN.CMP.DEV.FIELD`)
- Actual-against-actual comparison (owned by `SCAN.CMP.ACT`)

## Requirements

- **REQ-SCAN.CMP.DEV.SURFCAD-001** When `compare.surface_cad@1` runs, the system shall create one `DeviationField` element recording method, search distance in metres, angle cutoff in radians, alignment ID and the actual, nominal and alignment version digests, and shall modify no source.
- **REQ-SCAN.CMP.DEV.SURFCAD-002** The command shall evaluate the actual in the named alignment by mapping actual points (and normals) through the alignment's transform before calling the primitive, and shall fail with `geo.frame.mismatch` if the nominal is not in the alignment's target frame.
- **REQ-SCAN.CMP.DEV.SURFCAD-003** When the actual is a mesh, vertices whose every incident triangle is flagged synthesized shall receive status `EXCLUDED` and no value.
- **REQ-SCAN.CMP.DEV.SURFCAD-004** When an actual selection is given, points outside it shall receive `EXCLUDED`; when a nominal selection is given, only the selected faces or triangles shall be projection targets.
- **REQ-SCAN.CMP.DEV.SURFCAD-005** The field digest shall be identical for in-memory and chunked actuals, for any chunk size and thread count.
- **REQ-SCAN.CMP.DEV.SURFCAD-006** Comparison of a 10⁸-point cloud against a CAD model of up to 10⁴ faces shall complete within 600 s and 16 GiB peak resident memory on reference hardware [unverified].
- **REQ-SCAN.CMP.DEV.SURFCAD-007** If a selection is stale against its target digest, then the command shall fail with `scan.cmp.stale_selection` and create no field.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/cmp/panel_scan` (scanned freeform panel with CAD and alignment), when `fid compare surface-cad --actual <c> --nominal <cad> --alignment <a> --method normal --max-distance "5 mm" --max-angle "45 deg" --json` runs, then one field is created whose values match the PRIM oracle bit-for-bit and whose provenance holds all digests.
- **AC-2** Given the same cloud stored out-of-core with chunk sizes 10⁴ and 10⁶ points, then field digests equal the in-memory digest.
- **AC-3** Given fixture `fixtures/scan/mesh_open_cube` with one filled face (synthesized), vertices only on that face are `EXCLUDED`.
- **AC-4** Given an STL nominal mesh of the same panel, the command infers nominal kind `mesh` and records it.
- **AC-5** (error path) Given a stale actual selection, the command fails with `scan.cmp.stale_selection` and journals nothing.
- **AC-6** (replay) Given the journal of AC-1, replay in exact mode gives `IDENTICAL`.
- **AC-7** (bench, advisory) Given `fixtures/scan/cmp/bench_1e8` against a 10⁴-face CAD, REQ-006 is measured and published.

## Design

- **Registry command:** `compare.surface_cad@1`, W2, D0. Params: `actual`, `nominal` (CAD body, face selection or mesh), `method` (`normal` | `shortest`), `max_distance` (authored length), `max_angle` (authored angle), `alignment`, `actual_selection?`, `nominal_selection?`. Result: field handle and counts of `VALID`, `NO_MATCH`, `EXCLUDED`.
- **Algorithm:** resolve the alignment transform (ALN.MODEL.ELEM); build the nominal target once (mesh BVH or CAD face set) restricted to the nominal selection; stream actual blocks in original order (GEO.OOC.STREAM); per block transform points (GEO.FRAME.APPLY), mark excluded points, call `deviation_batch` on the rest; fill one `FieldBuilder`; finish.
- **Errors:** `geo.frame.mismatch`, `scan.cmp.stale_selection`, `scan.cmp.invalid_parameter`, PRIM and FIELD errors passed through.
- **Determinism:** D0.
- **Performance budget:** REQ-006 [unverified].
- **Crate:** `fid-scan-cmp`.

## Test fixtures

`panel_scan` (seeded synthetic scan of a freeform CAD panel), `mesh_open_cube` (shared), `bench_1e8` (generated at CI time), CC0.

## Build prompt

> In the Rust crate `fid-scan-cmp`, register `compare.surface_cad@1` (W2, D0; CLI `fid compare surface-cad --actual <h> --nominal <h> --alignment <h> --method normal|shortest --max-distance L --max-angle A [--actual-selection <h>] [--nominal-selection <h>] [--json]`, MCP `compare_surface_cad`). Infer the nominal kind (CAD or mesh); restrict targets to the nominal selection; check selections are not stale (`scan.cmp.stale_selection`). Stream the actual in original index order (`PointSource` from GEO.OOC), transform each block by the alignment, mark `EXCLUDED` for points outside the actual selection and for mesh vertices whose incident triangles are all synthesized (SCAN.MESHOP.PROV flag), and call `dev::deviation_batch` for the rest into one `FieldBuilder`. Store the `DeviationField` with full provenance. Digest must not depend on chunking or threads.
>
> Constraints: C2 (CPU values only), CONSTITUTION §3, §8, KNP-1; sources never modified. Tests: AC-1..AC-7 (AC-7 advisory).

## Open questions

- Nominal STL meshes use this same command (kind inferred), as proposed by the parent.

## Children

_Leaf: none._
