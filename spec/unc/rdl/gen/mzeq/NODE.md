---
id: UNC.RDL.GEN.MZEQ
title: Minimum-zone equioscillation generators
level: 4
parent: UNC.RDL.GEN
kind: task
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.RDL.GEN-002, REQ-UNC.RDL.GEN-004]
covers: []
depends_on: [UNC.RDL.GEN.HPREC, UNC.RDL.FORMAT]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.RDL.GEN.MZEQ: Minimum-zone equioscillation generators

## Purpose

Builds minimum-zone (Chebyshev) test cases whose solution is known by construction. Choose the solution and the zone width, then place the extremal points so that they satisfy the optimality (equioscillation or KKT) conditions for that solution. All other points lie strictly inside the zone by a recorded margin. After rounding, the reference is re-solved in high precision on the constructed active set, and the KKT conditions are verified.

## Scope

**In:**
- Families `mz.line2d`, `mz.plane`, `mz.circle`, `mz.sphere`, `mz.cylinder` (cone later)
- Construction of active sets that provably satisfy optimality (multipliers positive, active points in a valid alternation pattern)
- Interior points with a minimum margin so the active set survives rounding
- High-precision re-solve on the active set and KKT verification after rounding

**Out:**
- Least-squares families (owned by `UNC.RDL.GEN.LSNULL`)

## Requirements

- **REQ-UNC.RDL.GEN.MZEQ-001** Each family shall construct an active set whose optimality conditions hold for the chosen solution, with every multiplier positive by at least a recorded margin.
- **REQ-UNC.RDL.GEN.MZEQ-002** Every inactive point shall lie inside the zone by at least 1e-6 times the zone width [unverified], so the active set is unchanged by rounding to 16 digits.
- **REQ-UNC.RDL.GEN.MZEQ-003** After rounding, the family shall re-solve on the active set in `Hp`, verify KKT conditions, and fail with `active_set_changed` if verification fails.

## Interfaces

- **Offers:** families registered for `UNC.RDL.GEN.CMD`.
- **Consumes:** `Hp` and rounding (UNC.RDL.GEN.HPREC); record format (UNC.RDL.FORMAT).

## Decomposition hints

LOW (2D line, plane, circle: classical alternation constructions), HIGH (sphere, cylinder: active sets of 4 to 5 or more points with multiplier checks), KKT (shared verification). Needs review by a Chebyshev-fitting expert.

## Open questions

- Valid active-set patterns for the cylinder need expert input (number and arrangement of contacts) [unverified].

## Children

_None yet: status `open`._
