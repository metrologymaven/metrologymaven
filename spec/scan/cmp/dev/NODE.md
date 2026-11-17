---
id: SCAN.CMP.DEV
title: Deviation primitive and surface comparison against CAD
level: 3
parent: SCAN.CMP
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-SCAN.CMP-001, REQ-SCAN.CMP-002, REQ-SCAN.CMP-003, REQ-SCAN.CMP-004, REQ-SCAN.CMP-012]
covers: [GOM.SRF.01, PW.CMP.01, SA.CAD.02, SA.SCN.02, PCD.LAS.02]
depends_on: [GEO.DIST, GEO.INDEX, GEO.OOC, GEO.FRAME, SCAN.MESHOP.PROV, ALN]
cli: [fid compare surface-cad]
mcp: [compare_surface_cad]
journal: [compare.surface_cad@1]
updated: 2026-11-16
---

# SCAN.CMP.DEV: Deviation primitive and surface comparison against CAD

## Purpose

The single definition of "signed deviation" in SCAN and the command that applies it to a whole data set against CAD. It owns the kernel deviation primitive (one actual point against a nominal surface, with method, search distance and angle cutoff), the `DeviationField` element that stores a full comparison with its provenance, and the CAD comparison command that produces it at 10⁸-point scale. SCAN.CMP's other children and SCAN.SECT, SCAN.EDGE and SCAN.THK all call this primitive.

## Scope

**In:**
- Kernel deviation primitive: method `normal` or `shortest`, maximum search distance, maximum normal-angle cutoff, sign rule (positive on the material-outside side of the nominal)
- Per-point status: `VALID`, `NO_MATCH`, `EXCLUDED` (for example vertices used only by synthesized triangles)
- `DeviationField` element: values, statuses, method and parameters, alignment ID, digests of actual, nominal and alignment versions
- `compare.surface_cad@1`: cloud or mesh against CAD faces or a nominal mesh, optionally restricted to a selection of actual points and of nominal faces
- Streaming over out-of-core clouds with results identical to in-memory runs
- CPU-only values of record; a field never accepts GPU-computed values

**Out** (and which node owns it instead):
- Actual-against-actual comparison (owned by `SCAN.CMP.ACT`)
- Band classification and colour binning (owned by `SCAN.CMP.BAND`)
- GPU preview and its cross-check (owned by `SCAN.CMP.GPUPV`)
- Closest-point projection itself (owned by `GEO.DIST`)

## Requirements

- **REQ-SCAN.CMP.DEV-001** The deviation primitive shall compute, on the CPU in f64, a signed deviation for an actual point against a nominal surface by the method named in an explicit parameter (`normal` or `shortest`), with the sign positive when the point lies on the material-outside side of the nominal.
- **REQ-SCAN.CMP.DEV-002** If no nominal point lies within the maximum search distance, or the angle between the actual normal (where present) and the nominal normal at the foot point exceeds the cutoff, then the primitive shall return status `NO_MATCH` and no numeric value.
- **REQ-SCAN.CMP.DEV-003** When `compare.surface_cad@1` runs, the system shall create a `DeviationField` element that records method, maximum search distance in metres, angle cutoff in radians, alignment ID, and the digests of the actual, nominal and alignment versions.
- **REQ-SCAN.CMP.DEV-004** When the actual is a mesh, vertices used only by triangles flagged synthesized shall receive status `EXCLUDED` and no value.
- **REQ-SCAN.CMP.DEV-005** The `DeviationField` element shall accept values only from the CPU kernel path, and the type system shall make it impossible to construct a field from a GPU preview buffer.
- **REQ-SCAN.CMP.DEV-006** Surface comparison shall be determinism class D0, with identical field digests for in-memory and chunked actual data and for any thread count.
- **REQ-SCAN.CMP.DEV-007** Comparison of a 10⁸-point cloud against a CAD model of up to 10⁴ faces shall complete within a published time and memory budget on reference hardware, initially 600 s and 16 GiB [unverified].

## Interfaces

- **Offers:** kernel API `deviation(point, normal?, nominal, params) -> DevResult` and its batch form; `DeviationField` element type (values, statuses, provenance); `compare.surface_cad@1` (W2).
- **Consumes:** GEO.DIST (closest point, signed distance, foot-point normal, no-hit result), GEO.INDEX and GEO.OOC (batching and streaming), GEO.FRAME (frame checks), SCAN.MESHOP.PROV (synthesized flag), ALN (the alignment in which comparison runs, by ID and digest).

## Decomposition hints

PRIM (the primitive and its sign and cutoff rules, with exhaustive edge and vertex tests), FIELD (`DeviationField` element and its canonical digest), SURFCAD (the command, selections, streaming and the performance budget).

## Open questions

- The exact meaning of method `normal` (projection along the nominal normal at the foot point versus a ray along the actual normal) must be fixed by PRIM and documented against incumbent behaviour [unverified].
- Whether nominal meshes (STL nominals) are compared through the same command or a separate `compare.surface_mesh@1`; proposed: same command, nominal type inferred.

## Children

_None yet: status `open`._
