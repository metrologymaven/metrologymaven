---
id: LVM.ADJ.DATUM
title: Datum definition, initial values and rank checks
level: 3
parent: LVM.ADJ
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-LVM.ADJ-004]
covers: []
depends_on: [LVM.ADJ.OBSEQ, LVM.STATION.MODEL, ALN.XFM]
cli: [fid net set-datum]
mcp: [net_set_datum]
journal: [net.set_datum@1]
updated: 2026-11-16
---

# LVM.ADJ.DATUM: Datum definition, initial values and rank checks

## Purpose

A network adjustment needs a datum (which coordinates are fixed or constrained) and starting values for every station and point. This node owns the datum definition command, the construction of initial values, and the check that stops the adjustment, naming the undetermined parameters, when the datum or the observations leave the network under-determined.

## Scope

**In:**
- Datum types: fixed station, fixed points (all or selected coordinates), minimal constraints, inner constraints (free network)
- `net.set_datum@1` command and datum validation
- Initial values: station initial poses from accepted moves, otherwise bootstrapped by point-pair transforms from already-posed stations; point initial values by intersection or from Cartesian observations
- Rank-deficiency detection before and during the solve, naming the undetermined parameters (for example "translation X", "rotation about Z", "scale", or a station or point ID)

**Out** (and which node owns it instead):
- Iteration and covariance (`LVM.ADJ.SOLVE`)
- Station-move solves and acceptance (`LVM.MOVE`)
- Point-pair transform solver (`ALN.XFM`)

## Requirements

- **REQ-LVM.ADJ.DATUM-001** When `net.set_datum@1` runs, the system shall store one datum definition of type fixed station, fixed points, minimal constraints or inner constraints, and validate that referenced stations and points exist.
- **REQ-LVM.ADJ.DATUM-002** If the datum plus all enabled observations leave any parameter undetermined, then the system shall stop with `NET_RANK_DEFICIENT` listing each undetermined datum degree of freedom or parameter by name and shall journal no result.
- **REQ-LVM.ADJ.DATUM-003** If a network has only direction-only observations and no distance or scale constraint, then the rank check shall name "scale" as undetermined.
- **REQ-LVM.ADJ.DATUM-004** When a station has no initial pose, the system shall bootstrap one by a point-pair transform from at least 3 non-collinear common points with an already-posed station, and shall fail with `NET_NO_INITIAL_VALUE` naming the station if that is impossible.
- **REQ-LVM.ADJ.DATUM-005** Under inner constraints, the adjusted coordinates shall have zero mean translation and rotation relative to the initial values over the selected points, within 1e-12 m [unverified].

## Interfaces

- **Offers:** `net.set_datum@1` (W2); `Datum` and `InitialValues` for `LVM.ADJ.SOLVE`; rank diagnostics.
- **Consumes:** `LVM.STATION.MODEL` initial-pose slots; `ALN.XFM` point-pair transforms; `LVM.ADJ.OBSEQ` Jacobians for the rank test.

## Decomposition hints

DEF (datum types and command), INIT (initial values and bootstrapping), RANK (rank and null-space diagnostics with parameter names).

## Open questions

- Which numerical rank test (pivoted QR, eigenvalue ratio) and tolerance should be the default? Must be deterministic under KNP-1.

## Children

_None yet: status `open`._
