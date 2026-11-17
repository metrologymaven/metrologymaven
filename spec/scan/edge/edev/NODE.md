---
id: SCAN.EDGE.EDEV
title: Edge comparison, station deviations and statistics
level: 3
parent: SCAN.EDGE
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.EDGE-001, REQ-SCAN.EDGE-002, REQ-SCAN.EDGE-003, REQ-SCAN.EDGE-005, REQ-SCAN.EDGE-006]
covers: [GOM.SRF.06, PW.CMP.04]
depends_on: [SCAN.EDGE.NOMEDGE, SCAN.EDGE.MESHBND, SCAN.CMP.CPT]
side_effect: W2
determinism: D0
cli: [fid compare edge-trim, fid compare edge-create-points]
mcp: [compare_edge_trim, compare_edge_create_points]
journal: [compare.edge_trim@1, compare.edge_create_points@1]
updated: 2026-11-16
---

# SCAN.EDGE.EDEV: Edge comparison, station deviations and statistics

## Purpose

The edge comparison command: given a nominal edge, actual data and an alignment, it builds stations (NOMEDGE), finds the actual edge at each station (MESHBND's `extract_edge`, with further methods from EST), and reports at each station the trim deviation (in the nominal surface, perpendicular to the edge) and the normal deviation (along the nominal surface normal) as separate signed values, plus per-edge statistics. It can also turn stations into toleranced comparison points that report through SCAN.CMP's comparison-point model.

## Scope

**In:**
- `compare.edge_trim@1`: edge-comparison element with per-station trim and normal deviation and status
- Per-edge min, max, mean and RMS of trim and normal deviation, `NO_MATCH` stations excluded and counted
- `compare.edge_create_points@1`: comparison points of kind `Edge` at chosen stations with trim or normal tolerances, evaluated through `SCAN.CMP.CPT`
- Recording of extraction method, parameters, spacing and source digests

**Out** (and which node owns it instead):
- Station placement and frames (owned by `SCAN.EDGE.NOMEDGE`)
- Finding the actual edge point (owned by `SCAN.EDGE.MESHBND`, plus `SCAN.EDGE.EST` for wrapped data, blocked on D-028)
- Comparison-point ResultRecords and decisions (owned by `SCAN.CMP.CPT`, `GDT.CHAR`, `UNC`)
- Drawing edge whiskers (owned by `VIZ.OVERLAY`)

## Requirements

- **REQ-SCAN.EDGE.EDEV-001** When `compare.edge_trim@1` runs, the system shall create an edge-comparison element recording the nominal edge reference, the actual and alignment digests, the alignment ID, the station spacing in metres and the extraction method ID and parameters, and shall modify no source.
- **REQ-SCAN.EDGE.EDEV-002** For each station with an actual edge point a, the system shall report trim deviation (a − p)·b and normal deviation (a − p)·n, where p is the station point and (t, n, b) its frame, as separate signed values in metres.
- **REQ-SCAN.EDGE.EDEV-003** For each station where extraction returns `NO_MATCH`, the system shall mark the station `NO_MATCH` and shall report no value for it.
- **REQ-SCAN.EDGE.EDEV-004** The system shall report per edge the min, max, mean and RMS of trim and of normal deviation over valid stations, and the count of `NO_MATCH` stations.
- **REQ-SCAN.EDGE.EDEV-005** When `compare.edge_create_points@1` runs, the system shall create comparison points of kind `Edge` at the chosen stations whose evaluation uses this node's trim or normal deviation, and whose ResultRecords are emitted by `SCAN.CMP.CPT`.
- **REQ-SCAN.EDGE.EDEV-006** On a synthetic fixture with a known trim offset of 0.5 mm and a free mesh boundary, the measured trim deviation shall match the offset within 2 µm at every station [unverified tolerance].
- **REQ-SCAN.EDGE.EDEV-007** Edge comparison shall be determinism class D0.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/edge_plate_trimmed` (a plate mesh whose +x edge is trimmed 0.5 mm short of nominal and raised 0.1 mm, free boundary, seed 53), when `fid compare edge-trim --edge e --actual mesh --alignment a1 --spacing 0.01 --search 0.003 --json` runs, then every station reports trim −0.500 mm ± 2 µm and normal +0.100 mm ± 2 µm, and the statistics match.
- **AC-2** Given fixture `fixtures/scan/edge_plate_notch` (the same with a 20 mm missing section), the stations inside the notch are `NO_MATCH`, excluded from statistics and counted.
- **AC-3** Given stations 3 and 7 of AC-1 and `fid compare edge-create-points --edge-comparison c --stations 3,7 --component trim --upper 0.0003 --lower -0.0003`, then evaluating them yields two ResultRecords with value −0.500 mm ± 2 µm and decision `NONCONFORMS` under the bound decision rule.
- **AC-4** (error path) Given an actual in a different frame, the command fails with `geo.frame.mismatch` and journals nothing.
- **AC-5** (replay) Exact replay of AC-1 yields `IDENTICAL`.

## Design

- **Registry commands** (W2, D0): `compare.edge_trim@1` (`edge` NominalEdge ref, `actual`, `alignment`, `spacing` m, `search` m, `method` extraction method ID, `method_params?`); `compare.edge_create_points@1` (`edge_comparison`, `stations` [u32], `component` `trim`|`normal`, `upper` m, `lower` m).
- **Data:** `EdgeComparison { stations: [EdgeStation { s_arc, p, t, n, b, actual?, trim?, normal?, status }], stats_trim, stats_normal, provenance }`; `ComparisonPointKind::Edge { comparison, station, component }` registered with SCAN.CMP.CPT.
- **Algorithm:** `nominal::stations` → `extract_edge` (from SCAN.EDGE.MESHBND; the mesh free-boundary method needs no D-028 choice) → dot products without FMA → statistics with pairwise summation.
- **Errors:** `geo.frame.mismatch`, `scan.edge.bad_spacing`, `scan.edge.unknown_method`, NOMEDGE and MESHBND errors passed through.
- **Determinism:** D0.
- **Performance budget:** 50 edges × 1 000 stations within 5 s [unverified].
- **Crate:** `fid-scan-edge`.

## Test fixtures

- `fixtures/scan/edge_plate_trimmed` (seed 53) and `fixtures/scan/edge_plate_notch`: generated mesh and STEP, CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-scan-edge`, implement two W2, D0 registry commands. `compare.edge_trim@1` (CLI `fid compare edge-trim --edge <h> --actual <h> --alignment <h> --spacing M --search M [--method ID] [--json]`, MCP `compare_edge_trim`): compute stations with `nominal::stations`, get the actual edge point per station from `extract::extract_edge` (initially only the `mesh_free_boundary` method), report trim = (a − p)·b and normal = (a − p)·n per station, NO_MATCH stations without values, and per-edge min/max/mean/RMS for both components with the NO_MATCH count; store an edge-comparison element recording nominal edge, actual and alignment digests, alignment ID, spacing and extraction method ID and parameters. `compare.edge_create_points@1` (CLI `fid compare edge-create-points`, MCP `compare_edge_create_points`): create comparison points of kind `Edge` at chosen stations with trim or normal tolerances; their ResultRecords are emitted through the SCAN.CMP comparison-point evaluation (`compare.point_evaluate@1`), which calls back into this node for the value.
>
> Constraints: CPU only, KNP-1 (no FMA, pairwise sums, no std HashMap/HashSet), no panic/unwrap/unsafe, sources never modified. Tests: trimmed plate (trim −0.5 mm, normal +0.1 mm, ±2 µm), notch NO_MATCH handling, edge comparison points nonconforming, frame mismatch error, exact replay IDENTICAL.

## Open questions

- Values on wrapped or rounded edges depend on D-028; until it is decided, only the mesh free-boundary method is available and the command rejects other method IDs with `scan.edge.unknown_method`.

## Children

_Leaf: none._
