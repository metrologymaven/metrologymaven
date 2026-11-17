---
id: SCAN.EDGE.EST
title: Edge estimation on wrapped meshes and clouds
level: 3
parent: SCAN.EDGE
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-SCAN.EDGE-003, REQ-SCAN.EDGE-004, REQ-SCAN.EDGE-006]
covers: [GOM.SRF.06, PW.CMP.04]
depends_on: [SCAN.EDGE.NOMEDGE, SCAN.EDGE.MESHBND, GEO.INDEX]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# SCAN.EDGE.EST: Edge estimation on wrapped meshes and clouds

## Purpose

Estimates where the actual part edge lies when the scan has no clean free boundary: point clouds, and meshes that wrap around a rounded, chamfered or burred edge. It adds further extraction methods behind the same `extract_edge` interface that `SCAN.EDGE.MESHBND` defines. It is blocked on D-028 because the definition of the actual edge on such data changes the reported values and needs user input.

## Scope

**In:**
- Edge estimation methods for clouds and wrapped meshes, as chosen in D-028 (for example tangency break, curvature threshold, projected silhouette)
- Per-station actual edge point within the search distance, or `NO_MATCH`
- Method IDs and parameters recorded with every result; defaults per defaults profile if D-028 so decides

**Out** (and which node owns it instead):
- The extraction interface and the free-boundary method (owned by `SCAN.EDGE.MESHBND`)
- Nominal edge chain and station frames (owned by `SCAN.EDGE.NOMEDGE`)
- Deviation decomposition, statistics and the command (owned by `SCAN.EDGE.EDEV`)
- Gap and flush and hem analysis (owned by `IND`)

## Requirements

- **REQ-SCAN.EDGE.EST-001** For each nominal station, each estimation method shall return one actual edge point found within the stated search distance in metres in the station's normal plane, or `NO_MATCH` with no point.
- **REQ-SCAN.EDGE.EST-002** Each method shall apply the edge definition selected by D-028 and shall be registered under a method ID whose parameters are recorded with every result.
- **REQ-SCAN.EDGE.EST-003** Estimation shall be determinism class D0.
- **REQ-SCAN.EDGE.EST-004** On a synthetic fixture with a known trim offset and a stated edge radius, each method shall reproduce the offset within a published tolerance [unverified].

## Interfaces

- **Offers:** additional `EdgeMethod` implementations for `extract_edge` (interface owned by `SCAN.EDGE.MESHBND`), consumed by `SCAN.EDGE.EDEV`.
- **Consumes:** SCAN.EDGE.NOMEDGE (stations and frames), SCAN.EDGE.MESHBND (interface), GEO.INDEX (neighbour queries).

## Decomposition hints

One leaf per method accepted by D-028, plus a shared fixture generator for rounded and burred edges.

## Open questions

- **Unblocked (2026-11-17):** D-028 (Definition of the actual edge for trim-line deviation) is accepted. Apply its Decision section (`decisions/D-028-*.md`) when this node is expanded.
- Blocked on D-028: definition of the actual edge on rounded, chamfered or burred edges.

## Children

_None yet: status `decision`._
