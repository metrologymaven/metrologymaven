---
id: UNC.RDL.GEN
title: Tier G synthetic generators and oracle cross-checks
level: 3
parent: UNC.RDL
kind: module
status: open
phase: P0
size: XL
owners: []
claimed_by:
satisfies: [REQ-UNC.RDL-003, REQ-UNC.RDL-004]
covers: []
depends_on: [UNC.RDL.FORMAT]
cli: [fid rdl generate-dataset]
mcp: [rdl_generate_dataset]
journal: [rdl.generate_dataset@1]
updated: 2026-11-16
---

# UNC.RDL.GEN: Tier G synthetic generators and oracle cross-checks

## Purpose

Generates reference datasets whose answers are known by construction: least-squares cases built by the null-space method (choose the solution, then build residuals orthogonal to the Jacobian's range at it), minimum-zone cases built from constructed equioscillation sets, partial features, extreme aspect ratios, and noise and outlier stress. Coordinates are rounded to 16 significant digits and references are recomputed for the rounded coordinates at 50 or more digits, then cross-checked by an independent oracle that lives outside the kernel.

## Scope

**In:**
- Generator families per fit aim (line, plane, circle, sphere, cylinder, cone; LS and minimum zone first)
- Seeded determinism and the generator record (method, tool version, seed)
- High-precision recomputation for rounded coordinates
- Recording oracle cross-check results in the dataset
- `rdl.generate_dataset@1` (R1; returns records, does not store them)

**Out:**
- Oracle implementations (separate `fiducial-oracles` repository, referenced by provenance)
- Freezing generated sets (owned by `UNC.RDL.FREEZE`)
- Property tests that generate fresh cases per CI run (owned by `KRN` test harness, which calls these generators)

## Requirements

- **REQ-UNC.RDL.GEN-001** Each generator shall produce byte-identical records for the same generator version and seed.
- **REQ-UNC.RDL.GEN-002** Each generator shall round input coordinates to 16 significant decimal digits and compute reference outputs for the rounded coordinates to at least 50 significant digits.
- **REQ-UNC.RDL.GEN-003** Each generated reference shall be cross-checked by at least one oracle independent of the generator, and the dataset shall record the oracle ID, version and agreement, failing generation if they disagree beyond the declared test uncertainty.
- **REQ-UNC.RDL.GEN-004** Least-squares generators shall use the null-space construction, and minimum-zone generators shall construct sets whose extremal points provably satisfy the optimality conditions for the chosen solution.

## Interfaces

- **Offers:** generator families; `rdl.generate_dataset@1`; generated records.
- **Consumes:** `RdlDataset` format (UNC.RDL.FORMAT); oracle results (external repository).

## Decomposition hints

One leaf per family: LSNULL (least-squares null-space), MZEQ (minimum-zone equioscillation), PART (partial features), ASPECT (extreme aspect ratios), STRESS (noise and outliers), plus XCHECK (oracle cross-check recording).

## Open questions

- The high-precision arithmetic for generators lives outside the kernel (generators are not values of record); whether the generator tool is Rust with MPFR or Python with mpmath is left to the leaves, provided the oracle uses the other route.

## Children

_None yet: status `open`._
