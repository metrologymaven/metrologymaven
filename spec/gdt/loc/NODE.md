---
id: GDT.LOC
title: Orientation and location tolerances
level: 2
parent: GDT
kind: subsystem
status: split
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-GDT-004, REQ-GDT-007]
covers: [GOM.GDT.05, GOM.GDT.06, GOM.GDT.07, GOM.GDT.08, GOM.GDT.09, GOM.GDT.10, PW.GDT.03, PW.GDT.04]
depends_on: [GDT.ZONE, GDT.DRF, GDT.RES]
cli: [fid check *-parallelism, fid check *-perpendicularity, fid check *-angularity, fid check *-position, fid check *-coaxiality, fid check *-concentricity, fid check *-symmetry]
mcp: [check_*_parallelism, check_*_perpendicularity, check_*_angularity, check_*_position, check_*_coaxiality, check_*_concentricity, check_*_symmetry]
journal: [check.*_parallelism, check.*_perpendicularity, check.*_angularity, check.*_position, check.*_coaxiality, check.*_concentricity, check.*_symmetry]
updated: 2026-11-16
---

# GDT.LOC: Orientation and location tolerances

## Purpose

Evaluates the datum-related tolerances that control orientation and location of features: parallelism, perpendicularity, angularity, position (single features and patterns), coaxiality and concentricity, and symmetry. They share one pattern: extract the toleranced feature (surface, derived axis, median plane or median points), express it in the DRF, and hand the zone to `GDT.ZONE`. Position of holes and pins, RFS and MMC, is the centre of the P1a product (CONSTITUTION §7 P1a).

## Scope

**In:**
- Parallelism, perpendicularity, angularity of planes, lines and axes to planar and axial datums
- Position of features of size and patterns of features, RFS, MMC and LMC (via `GDT.ZONE`), with surface and axis interpretation variants as named by the DMT row
- Coaxiality and concentricity (dialect-specific: concentricity rows exist for ASME 2018 only if the DMT says so [unverified: ASME Y14.5-2018 removed concentricity and symmetry symbols])
- Symmetry (ISO; ASME status per DMT)
- Derived-feature extraction: axes, median lines, median surfaces, median points, per DMT chain
- Location result payload: value, per-axis deviations in the DRF, actual and virtual condition, bonus

**Out** (and which node owns it instead):
- Zone pose solve, bonus, patterns, composite tiers (owned by `GDT.ZONE`)
- DRF and mobility (owned by `GDT.DRF`)
- Profile used as a location control (owned by `GDT.PROF`)
- Runout (owned by `GDT.PROF`)
- Form (owned by `GDT.FORM`)

## Requirements

- **REQ-GDT.LOC-001** The system shall evaluate parallelism, perpendicularity and angularity of a planar, linear or axial toleranced feature to a DRF and report the orientation value and the toleranced element used.
- **REQ-GDT.LOC-002** The system shall evaluate position of a single feature of size and of a pattern, RFS, MMC and LMC, and report the position value, per-axis deviations in the DRF, and stated and bonus tolerance.
- **REQ-GDT.LOC-003** When the DMT row for a position callout offers surface and axis interpretation variants, the system shall evaluate the variant selected by the resolver, and when both are requested it shall report both, with the row's designated deciding variant marked.
- **REQ-GDT.LOC-004** The system shall evaluate coaxiality, concentricity and symmetry only for the dialect and edition combinations the DMT marks supported, and shall return `UNSUPPORTED` otherwise.
- **REQ-GDT.LOC-005** When a derived feature (axis, median line, median surface or median points) is the toleranced element, the system shall derive it with the reduction operator named in the chain and record that operator's algorithm ID.
- **REQ-GDT.LOC-006** Orientation and position values shall match every ORS-GDT orientation and position case within the case's stated tolerance, and the P1a subset (orientation to planar datums, position of holes and pins RFS and MMC without mobility) shall be green before P1a exit.

## Interfaces

- **Offers:** `check.eval_<symbol>@1` for each symbol in Scope (R1 scratch, W2 when committed).
- **Consumes:** zone engine (GDT.ZONE); DRF (GDT.DRF); operator chain (GDT.RES); fitted features and derived-feature reductions (KRN); ResultRecord builder (GDT.CHAR).

## Decomposition hints

ORIENT (parallelism, perpendicularity, angularity), POS (single and pattern position), DERIV (derived feature extraction), COAX (coaxiality, concentricity), SYM (symmetry).

## Open questions

- Whether to evaluate concentricity / symmetry under ASME at all for 2018-governed drawings (legacy drawings may still use earlier editions); needs a DMT-level expert call. Routed to the DMT reviewers (D-009) through `GDT.LOC.COAX` and `GDT.LOC.SYM`, both P1b.

## Split rationale

The dividing principle is **shared front end, then symbol families**: derived-feature extraction (DERIV) and the common payload (LRES) are used by every symbol; then orientation (ORIENT), position (POS), coaxiality and concentricity (COAX), and symmetry (SYM). ORIENT and POS carry the P1a subset; COAX and SYM depend on ISO rows and an expert call on ASME, so they are P1b.

Alternatives considered:
- **Hint split ORIENT / POS / DERIV / COAX / SYM.** Adopted, plus LRES: the payload and DRF-deviation helpers are pure and small, so they are finished as a leaf now and keep the symbol modules thin.
- **Merge COAX and SYM.** Rejected: different derived elements (axes versus median points and surfaces) and zone shapes.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| GDT.LOC.LRES | Location and orientation result payload (leaf) | 001 002 | none |
| GDT.LOC.DERIV | Derived-feature extraction | 005 | none |
| GDT.LOC.ORIENT | Parallelism, perpendicularity and angularity | 001 006 | GOM.GDT.05, GOM.GDT.06, GOM.GDT.07, PW.GDT.03 |
| GDT.LOC.POS | Position of features and patterns | 002 003 006 | GOM.GDT.08, PW.GDT.04 |
| GDT.LOC.COAX | Coaxiality and concentricity | 004 | GOM.GDT.09 |
| GDT.LOC.SYM | Symmetry | 004 | GOM.GDT.10 |
