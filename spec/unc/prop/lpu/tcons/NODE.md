---
id: UNC.PROP.LPU.TCONS
title: Tangents for kernel constructions and evaluation operators
level: 4
parent: UNC.PROP.LPU
kind: feature
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.PROP.LPU-001]
covers: []
depends_on: [UNC.PROP.LPU.TAN, KRN.CONS.OPS.TABLE, KRN.CONS.ELEM.PRIM, KRN.CONS.DERIV.SYS]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.PROP.LPU.TCONS: Tangents for kernel constructions and evaluation operators

## Purpose

Closed-form Jacobians, registered in the `UNC.PROP.LPU.TAN` table, for every P1a kernel operator that is not a fit and not the formula IR: constructions (intersections, projections, offsets, midpoints and bisectors, definitional primitives, system elements) and kernel-side characteristic evaluation operators (distances, angles, sizes). Branch selectors and tangency thresholds of constructions become kink hints.

## Scope

**In:**
- Analytic Jacobians for each P1a variant in the `KRN.CONS.OPS.TABLE` support table, the definitional primitive builds and system elements
- Kink hints where a construction is near its `IllConditioned` threshold (tangency, near-parallel)
- Kernel-side evaluation operators present in the kernel manifest

**Out:**
- The contract and table (owned by `UNC.PROP.LPU.TAN`)
- Formula IR tangents (owned by `UNC.PROP.LPU.TFORM`)
- Fit sensitivities (owned by `UNC.PROP.LPU.IMPL` and `UNC.PROP.LPU.ASET`)

## Requirements

- **REQ-UNC.PROP.LPU.TCONS-001** For every P1a construction variant in the kernel support table, the module shall register a tangent provider that passes the `UNC.PROP.LPU.TAN` harness.
- **REQ-UNC.PROP.LPU.TCONS-002** When a construction input is within 10 times its tangency or parallelism threshold [unverified], the provider shall return a kink hint with the measured margin.
- **REQ-UNC.PROP.LPU.TCONS-003** When the coverage report runs on the P1a manifest, it shall list no construction or kernel evaluation operator as missing.

## Interfaces

- **Offers:** tangent providers registered in `TangentTable`.
- **Consumes:** construction algorithm definitions (KRN.CONS.*); the tangent contract (UNC.PROP.LPU.TAN).

## Decomposition hints

INTER (intersections), PROJ (projections and offsets), MID (midpoints and bisectors), PRIM (definitional builds and system elements), EVAL (kernel evaluation operators).

## Open questions

- Which characteristic-evaluation operators are kernel operators (and so in this node) versus GDT-side code is unconfirmed; GDT owners should confirm (cross-domain).

## Children

_None yet: status `open`._
