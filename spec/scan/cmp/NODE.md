---
id: SCAN.CMP
title: Surface comparison and comparison points
level: 2
parent: SCAN
kind: subsystem
status: split
phase: P1b
size: XL
owners: []
claimed_by:
satisfies: [REQ-SCAN-001, REQ-SCAN-002, REQ-SCAN-006, REQ-SCAN-007]
covers: [GOM.SRF.01, GOM.SRF.02, GOM.SRF.03, GOM.SRF.09, GOM.SRF.10, PW.CMP.01, PW.CMP.03, PW.CMP.06, SA.CAD.02, SA.SCN.02, PCD.LAS.02]
depends_on: [GEO, ALN, GDT]
cli: [fid compare surface-*, fid compare point-*, fid compare extreme-*]
mcp: [compare_surface_*, compare_point_*, compare_extreme_*]
journal: [compare.surface_*, compare.point_*, compare.extreme_*]
updated: 2026-11-16
---

# SCAN.CMP: Surface comparison and comparison points

## Purpose

Computes the values of record behind every scan-to-CAD color map: the signed per-point deviation of measured data (cloud or mesh) against a nominal (CAD or another actual), in a named alignment. On top of that deviation field it provides tolerance-band classification, deviation labels, comparison points with tolerances that become ResultRecords, and the max/min deviation search. It also owns the shared **deviation primitive** that SCAN.SECT, SCAN.EDGE and SCAN.THK reuse, so there is exactly one definition of "signed deviation" in SCAN.

## Scope

**In:**
- The deviation primitive: signed deviation of a point against a nominal, along the nominal normal or as shortest distance, with maximum search distance and maximum normal-angle cutoff
- Surface comparison of actual data against CAD, and of actual against actual (for example scan against reference scan)
- The deviation field element: per-point value, method, reference element versions, alignment ID, and per-point status for points with no valid match
- Tolerance-band classification of a deviation field (values supplied to VIZ legends)
- Deviation labels: user-placed or automatic points that read the deviation field and display value and tolerance
- Comparison points with nominal position, direction and tolerance, producing ResultRecords
- Max and min deviation search over a field or a region, with optional minimum separation between reported extremes
- Cloud-to-CAD comparison as listed for PC-DMIS laser and SpatialAnalyzer workflows

**Out** (and which node owns it instead):
- Rendering color maps, legends and labels (owned by `VIZ`)
- Section curves and whiskers (owned by `SCAN.SECT`)
- Edge and trim-line deviation (owned by `SCAN.EDGE`)
- Wall and material thickness (owned by `SCAN.THK`)
- Profile-of-a-surface tolerance evaluation (owned by `GDT`)
- Conformance decision rules and uncertainty (owned by `UNC`, applied through `GDT`)
- Closest-point projection and distance queries (owned by `GEO`; this node composes them)

## Requirements

- **REQ-SCAN.CMP-001** The deviation primitive shall compute, on the CPU, a signed deviation per actual point along the nominal surface normal or as the signed shortest distance, as chosen by an explicit parameter, with the sign positive on the material-outside side of the nominal.
- **REQ-SCAN.CMP-002** If no nominal point lies within the stated maximum search distance, or the angle between actual and nominal normals exceeds the stated cutoff, then the deviation primitive shall mark that point `NO_MATCH` and shall not report a numeric value for it.
- **REQ-SCAN.CMP-003** When a surface comparison runs, the system shall create a deviation field element that records the method, maximum search distance, angle cutoff, alignment ID, and digests of the actual, nominal and alignment versions.
- **REQ-SCAN.CMP-004** Deviation values of record shall be computed only by the CPU kernel, and any GPU-computed deviation shall be labelled preview and shall never be written to a deviation field, report or ResultRecord.
- **REQ-SCAN.CMP-005** A GPU preview deviation field, where offered, shall agree with the CPU field within a published tolerance on reference fixtures [unverified].
- **REQ-SCAN.CMP-006** Surface comparison against actual data shall use the same deviation primitive and field element as comparison against CAD.
- **REQ-SCAN.CMP-007** The system shall classify each value of a deviation field into tolerance bands from a stated band set, and shall report the count and area fraction per band.
- **REQ-SCAN.CMP-008** A comparison point shall hold a nominal position, a nominal direction, a deviation type (normal, or along an axis) and upper and lower tolerances, and evaluating it shall emit a ResultRecord with `evaluation_basis` `POINTS`.
- **REQ-SCAN.CMP-009** Comparison-point ResultRecords shall carry uncertainty UEC-0 unless UNC supplies an evaluated uncertainty, and the decision shall come from the decision rule named on the characteristic.
- **REQ-SCAN.CMP-010** Deviation labels shall be derived read-only from a deviation field or a comparison point, and shall update when that source recomputes.
- **REQ-SCAN.CMP-011** The max/min search shall report the N largest positive and N largest negative deviations in a field or region, separated by at least a stated distance in metres, with position and value of each.
- **REQ-SCAN.CMP-012** Surface comparison of a 10⁸-point cloud against a CAD model of up to 10⁴ faces shall complete within a published time and memory budget on reference hardware [unverified].

## Interfaces

- **Offers:** `compare.surface_*`, `compare.point_*` and `compare.extreme_*` commands; the `DeviationField` element; the deviation primitive (kernel API) used by SCAN.SECT, SCAN.EDGE and SCAN.THK; comparison-point characteristics and their ResultRecords; band classifications for VIZ.
- **Consumes:** GEO (closest-point projection to mesh and CAD face, spatial index, selections), ALN (the alignment in which comparison runs), GDT (characteristic and ResultRecord model), SCAN.MESHOP (synthesized-triangle flags to exclude from comparison).

## Decomposition hints

DEV (deviation primitive and field), ACT (actual-to-actual comparison), BAND (tolerance bands), CPT (comparison points and ResultRecords), LABEL (deviation labels), EXTR (max and min search), GPUPV (GPU preview with CPU cross-check).

## Open questions

- Sign convention for actual-to-actual comparison when the reference is a cloud without a material side: proposed rule is the reference normal direction; confirm with users.
- Whether comparison points should be GDT characteristics (owned by GDT) or SCAN characteristics reported through GDT's result model. This node assumes the latter.

## Split rationale

The split puts the **value of record** first: DEV owns the one deviation primitive, the `DeviationField` element and the CAD comparison at full scale. Everything else is a consumer of a field with one clear job: actual-to-actual comparison through the same primitive (ACT), kernel-side band and colour binning that VIZ.OVERLAY draws verbatim (BAND), toleranced comparison points reported through GDT.CHAR (CPT), read-only labels (LABEL), extreme search (EXTR), and the GPU preview that must never become a value of record (GPUPV). ACT, BAND, CPT, LABEL and EXTR are leaves in `fid-scan-cmp`; DEV and GPUPV stay open (method details, out-of-core scale, GPU path).

Alternatives considered:
- **Merge LABEL into CPT.** Rejected: labels have no tolerance, no decision and no ResultRecord; merging would blur what is reported.
- **Put binning in VIZ or KRN.** Rejected: VIZ must never compute a bin (REQ-VIZ.OVERLAY-001) and the band set is specific to deviation and thickness fields; the boundary agreed at review puts kernel-side binning here.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| SCAN.CMP.DEV | Deviation primitive and surface comparison against CAD | 001 002 003 004 012 | GOM.SRF.01, PW.CMP.01, SA.CAD.02, SA.SCN.02, PCD.LAS.02 |
| SCAN.CMP.ACT | Actual-to-actual surface comparison | 006 | GOM.SRF.02 |
| SCAN.CMP.BAND | Tolerance bands and kernel colour binning | 007 | GOM.SRF.10 |
| SCAN.CMP.CPT | Comparison points and their ResultRecords | 008 009 | PW.CMP.03 |
| SCAN.CMP.LABEL | Deviation labels | 010 | GOM.SRF.03, PW.CMP.06 |
| SCAN.CMP.EXTR | Maximum and minimum deviation search | 011 | GOM.SRF.09 |
| SCAN.CMP.GPUPV | GPU deviation preview with CPU cross-check | 004 005 | none |
