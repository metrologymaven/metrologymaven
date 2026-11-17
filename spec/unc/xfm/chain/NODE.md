---
id: UNC.XFM.CHAIN
title: Covariance along chained transforms
level: 3
parent: UNC.XFM
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.XFM-003]
covers: [SA.UNC.03]
depends_on: [UNC.XFM.TCOV, UNC.XFM.APPLY]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# UNC.XFM.CHAIN: Covariance along chained transforms

## Purpose

Accumulates transform covariance along alignment hierarchies and frame chains (part to fixture to machine, station to station), keeping the correlation between transforms that were estimated from shared observations. Treating chained transforms as independent understates or overstates uncertainty when they share data, which is common in pre-alignment followed by fine alignment on overlapping points.

## Scope

**In:**
- Composition of covariances under the left-perturbation convention (adjoint mapping)
- Joint covariance of transforms that share observations, obtained from a joint linearisation or joint Monte Carlo
- Chain graph bookkeeping: which observations feed which transform

**Out:**
- Single-transform covariance (owned by `UNC.XFM.TCOV`)
- Network adjustment of station poses (owned by `LVM`)

## Requirements

- **REQ-UNC.XFM.CHAIN-001** When two transforms are composed, the system shall propagate their joint covariance through the composition using the adjoint of the first transform.
- **REQ-UNC.XFM.CHAIN-002** When two transforms in a chain were estimated from overlapping observation sets, the system shall compute and use their cross-covariance, and shall never assume independence without recording `assumed_independent`.
- **REQ-UNC.XFM.CHAIN-003** For a chain of 5 transforms with shared observations, the propagated covariance shall agree with a joint Monte Carlo within 10 % on every standard deviation [unverified].

## Interfaces

- **Offers:** `compose_with_covariance(chain) -> TransformCovariance`; joint covariance for transform groups.
- **Consumes:** TCOV outputs and observation provenance from ALN.

## Decomposition hints

COMPOSE (adjoint composition), JOINT (cross-covariance from shared observations), GRAPH (chain bookkeeping).

## Open questions

- Joint linearisation needs ALN to expose which observations each transform used; confirm with ALN owners.

## Children

_None yet: status `open`._
