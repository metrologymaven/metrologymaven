---
id: GDT.DIM.SIZEASME.LOCAL
title: ASME two-point local size under a DMT pairing rule
level: 4
parent: GDT.DIM.SIZEASME
kind: feature
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.DIM.SIZEASME-001, REQ-GDT.DIM.SIZEASME-004]
covers: [PW.GDT.01]
depends_on: [GDT.RES.RESOLVE, KRN.LSQ]
uses_artifacts: [GDT.RES.ROWASME, UNC.ORS.SIZE]
cli: []
mcp: []
journal: []
updated: 2026-11-22
---

# GDT.DIM.SIZEASME.LOCAL: ASME two-point local size under a DMT pairing rule

## Purpose

Computes the set of two-point local sizes of an ASME Y14.5-2018 feature of size (cylinder, sphere, two opposite parallel planes) and their minimum and maximum. Which points are paired, in which cross-sections and against which reference (the pairing rule) is interpretation content that expert-authored DMT rows in `GDT.RES.ROWASME` define (D-009, texts per D-016); this node executes the rule the resolved chain names and never invents one.

## Scope

**In:**
- Executing the pairing operator named by the resolved chain for cylinders, spheres and opposite parallel planes
- Cross-section extraction where the chain names it
- Output `LocalSizes { sizes: [f64], pairs: [(i, j)], min, max, pairing_alg }`
- Matching every ORS-SIZE ASME local-size case

**Out** (and which node owns it instead):
- Envelope and Rule #1 containment (owned by `GDT.DIM.SIZEASME.ENVLP`)
- The size command and payload (owned by `GDT.DIM.SIZEASME.CMD`)
- Pairing rules as content (owned by DMT rows in `GDT.RES.ROWASME`)
- ISO LP and LS (owned by `GDT.DIM.SIZEISO.LOCAL`)

## Requirements

- **REQ-GDT.DIM.SIZEASME.LOCAL-001** When the resolved chain names a pairing operator, the system shall compute every two-point local size the operator defines and report the minimum, the maximum and the index pair attaining each.
- **REQ-GDT.DIM.SIZEASME.LOCAL-002** If the chain names a pairing operator that is not implemented, then the system shall return `UNSUPPORTED` with the operator's algorithm ID and compute nothing.
- **REQ-GDT.DIM.SIZEASME.LOCAL-003** If a cross-section or point has no partner under the operator, then the system shall exclude it, count it in `unpaired`, and fail with `insufficient_pairs` when fewer than the row's minimum pairs remain.
- **REQ-GDT.DIM.SIZEASME.LOCAL-004** Local sizes shall match every ORS-SIZE ASME local-size case within the case's stated tolerance.

## Interfaces

- **Offers:** `local_sizes(points, feature_kind, chain) -> LocalSizes` to `GDT.DIM.SIZEASME.CMD`; also usable by `GDT.DIM.SIZEISO.LOCAL` if an ISO row names the same operator.
- **Consumes:** resolved chain (GDT.RES.RESOLVE); least-squares reference features for section orientation where the chain names them (KRN.LSQ); DMT rows (`GDT.RES.ROWASME`, read as data); ORS-SIZE cases (`UNC.ORS.SIZE`, read as data).

## Decomposition hints

PAIRMECH (a closed enum of pairing operators as plain geometry once rows name them; a leaf), XSECT (cross-section extraction for cylinders), WIDTH (opposite parallel planes). Only the operators the signed-off rows name should be implemented.

## Open questions

- Not a leaf: the pairing rule is interpretation content awaiting signed-off rows in `GDT.RES.ROWASME` (D-009 authorship, D-016 text access now accepted). Agents must not pick a default.

## Children

_None yet: status `open`._
