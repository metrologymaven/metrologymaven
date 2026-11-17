---
id: SCAN.EDGE
title: Edge and trim-line deviation
level: 2
parent: SCAN
kind: subsystem
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-SCAN-001, REQ-SCAN-002, REQ-SCAN-005]
covers: [GOM.SRF.06, PW.CMP.04]
depends_on: [SCAN.CMP, GEO]
cli: [fid compare edge-*]
mcp: [compare_edge_*]
journal: [compare.edge_*]
updated: 2026-11-16
---

# SCAN.EDGE: Edge and trim-line deviation

## Purpose

Measures where the actual boundary of a part lies compared with its nominal boundary: trimmed sheet-metal edges, cut lines and free boundaries. It extracts actual edge points from scan data near a nominal CAD edge and reports in-surface (trim) and normal (height) deviation along the edge. It is separate because boundary extraction from scans is a distinct problem from interior surface comparison, with its own failure modes (missing data, rounded edges, burrs).

## Scope

**In:**
- Nominal edge selection from CAD boundary edges or curves
- Actual edge extraction from a mesh boundary or from a cloud near the nominal edge
- Edge deviation along the nominal edge at a stated spacing, decomposed into in-surface (trim) and surface-normal components
- Edge statistics: min, max, mean and RMS per edge, with stations lacking data counted
- Edge comparison points that produce ResultRecords via SCAN.CMP's comparison-point model

**Out** (and which node owns it instead):
- Interior surface deviation and comparison-point model (owned by `SCAN.CMP`)
- Gap and flush between two parts, and hem analysis (owned by `IND`)
- Rendering of edge whiskers (owned by `VIZ`)

## Requirements

- **REQ-SCAN.EDGE-001** When an edge comparison runs, the system shall create an edge-comparison element recording the nominal edge, actual source digests, alignment ID, station spacing in metres and extraction method, without modifying any source.
- **REQ-SCAN.EDGE-002** At each station along the nominal edge, the system shall report the trim deviation in the nominal surface tangent plane, perpendicular to the edge, and the normal deviation along the nominal surface normal, as separate signed values.
- **REQ-SCAN.EDGE-003** If no actual boundary is found within a stated search distance of a station, then the system shall mark that station `NO_MATCH` and shall not report a value for it.
- **REQ-SCAN.EDGE-004** The edge extraction method (mesh boundary or cloud boundary estimation) and its parameters shall be documented and recorded with the result, and the extraction shall be determinism class D0.
- **REQ-SCAN.EDGE-005** The system shall report min, max, mean and RMS of trim and normal deviation per edge, excluding and counting `NO_MATCH` stations.
- **REQ-SCAN.EDGE-006** On a synthetic fixture with a known trim offset, the measured trim deviation shall match the offset within a published tolerance [unverified].

## Interfaces

- **Offers:** `compare.edge_*` commands (for example `compare.edge_trim@1`); edge-comparison elements; edge stations as comparison points.
- **Consumes:** SCAN.CMP (deviation primitive, comparison-point model), GEO (CAD edges, mesh boundary loops, spatial index).

## Decomposition hints

NOMEDGE (nominal edge selection), EXTRACT (actual boundary extraction), EDEV (station deviations and statistics).

## Open questions

- How a rounded or burred edge is defined for extraction (tangency break, curvature threshold, or projected silhouette) needs input from sheet-metal users: opened as D-028, blocking `SCAN.EDGE.EST` only.

## Split rationale

The split separates what is **independent of the edge-definition question** from what depends on it. NOMEDGE (station placement and the t, n, b frame on exact CAD) and MESHBND (the extraction interface plus the free-boundary method, where the mesh boundary is unambiguously the edge) are leaves; EDEV (the command, trim and normal decomposition, statistics, edge comparison points) is a leaf that works with any extraction method. EST holds the methods for wrapped meshes and clouds and is blocked on D-028, because how a rounded or burred edge is defined changes the reported values and needs sheet-metal users' input.

Alternatives considered:
- **Block the whole of SCAN.EDGE on the edge definition.** Rejected: free-boundary trim deviation is well defined and is the common case for one-sided sheet scans.
- **Merge NOMEDGE into EDEV.** Rejected: EST and MESHBND need the same station frames, and the frame math on exact CAD is separately testable.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| SCAN.EDGE.NOMEDGE | Nominal edge chains and station frames | 001 002 | none |
| SCAN.EDGE.MESHBND | Edge extraction interface and mesh free-boundary method | 003 004 006 | GOM.SRF.06, PW.CMP.04 |
| SCAN.EDGE.EST | Edge estimation on wrapped meshes and clouds (decision D-028) | 003 004 006 | GOM.SRF.06, PW.CMP.04 |
| SCAN.EDGE.EDEV | Edge comparison, station deviations and statistics | 001 002 003 005 006 | GOM.SRF.06, PW.CMP.04 |
