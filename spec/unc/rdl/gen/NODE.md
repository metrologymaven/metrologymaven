---
id: UNC.RDL.GEN
title: Tier G synthetic generators and oracle cross-checks
level: 3
parent: UNC.RDL
kind: module
status: split
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
updated: 2026-11-17
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

- **Applied (arithmetic route):** generators are Rust in crate `fid-rdl-gen`, using a pure-Rust arbitrary-precision float library whose licence must be on the D-001 allowlist [unverified: library choice]. The independent oracle (external `fiducial-oracles` repository) uses Python with mpmath, the other route. `fid-rdl-gen` never links the kernel's fit code; it may use the kernel's counter-based RNG for seeding only.
- **Layering:** RDL data and generators are tooling for kernel validation, but `rdl.generate_dataset@1` is a registry command, so `UNC.RDL.GEN.CMD` sits in the UNC-commands part of L2 and depends on CORE bus nodes. Other nodes consume generated data only as artifacts (frozen releases).
- Oracle results enter as files (artifact of the external repository). Records whose cross-check is `pending` must not be frozen; `UNC.RDL.FREEZE` should check this (cross-node note).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| UNC.RDL.GEN.HPREC | High-precision arithmetic, rounding and generator record (leaf) | 001 002 | none |
| UNC.RDL.GEN.LSNULL | Least-squares null-space generators for six primitives (leaf) | 002 004 | none |
| UNC.RDL.GEN.MZEQ | Minimum-zone equioscillation generators | 002 004 | none |
| UNC.RDL.GEN.GEOM | Partial-coverage and extreme-aspect-ratio families (leaf) | 004 | none |
| UNC.RDL.GEN.STRESS | Noise-level and outlier stress families (leaf) | 004 | none |
| UNC.RDL.GEN.XCHECK | Oracle cross-check recording and gating (leaf) | 003 | none |
| UNC.RDL.GEN.CMD | The rdl.generate_dataset@1 command and family registry (leaf) | 001 | none |
