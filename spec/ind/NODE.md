---
id: IND
title: Industry analysis packs
level: 1
parent: FID
kind: domain
status: split
phase: P3
size: XL
owners: []
claimed_by:
satisfies: [REQ-FID-013]
covers: [GOM.SPE.*, PW.IND.*, PCD.IND.*]
depends_on: [GDT, SCAN, ECO]
cli: [fid pack *]
mcp: [pack_*]
journal: [pack.*]
updated: 2026-11-16
---

# IND: Industry analysis packs

## Purpose

Industry-specific analyses that ship as installable packs on public extension points, so domain experts in the community can own them:
- airfoil and blade;
- gear;
- sheet metal (holes, trim, springback, hem);
- gap and flush;
- pipe and tube;
- hole-pattern checks;
- portable-device workflows.

## Scope

**In:**
- The pack contract (what a pack may register: commands, element types, checks, report templates)
- The first-party reference packs listed above

**Out:**
- The generic plugin and SDK mechanism (ECO)
- Core GD&T (GDT)
- Core surface comparison (SCAN)

## Requirements

- **REQ-IND-001** An industry pack shall register its commands, element types, checks and report templates only through public extension points, with no private engine access.
- **REQ-IND-002** Each pack shall declare the standards and methods it implements, with designators and editions, and ship reference datasets with expected results.
- **REQ-IND-003** The project shall provide reference packs for airfoil, gear, sheet metal, gap and flush, and tube, each owned by a named maintainer.
- **REQ-IND-004** Pack results shall be ResultRecords carrying the pack's identity and version, and shall replay like any other result.
- **REQ-IND-005** If a pack is removed or disabled, then projects that used it shall still open, and its results shall be shown read-only with their provenance.

## Interfaces

- **Offers:** the pack contract; `pack.*` management commands; reference packs.
- **Consumes:** ECO SDK, GDT, SCAN.

## Decomposition hints

CONTRACT (pack contract), AIRFOIL, GEAR, SHEET (holes, trim, springback, hem), GAPFL (gap and flush), TUBE, PATTERN, PORT (portable workflows).

## Split rationale

The split is **one pack per child plus one contract child**. Each industry pack is the natural unit of ownership (REQ-IND-003 asks for a named maintainer per pack), of installation and of reference datasets, so each pack is its own subsystem. Everything the packs share (manifest, extension-point profile, conformance harness, provenance stamping, missing-pack behaviour) lives once in `IND.CONTRACT`, and every pack `depends_on` it. This keeps REQ-IND-001, 002, 004 and 005 owned by the contract, while each pack child only adds its own conformance and domain requirements.

Sheet-metal holes and trim stay in `IND.SHEET` because their definitions depend on sheet thickness and material side; generic batch recognition of repeated features goes to `IND.PATTERN` so it can serve any element type. Portable-device workflows (`IND.PORT`) are a pack because they are operator procedures layered on HAL sessions, not device I/O.

Alternatives considered:
- **Split by mechanism** (element types, checks, report templates, management). Rejected: it scatters each industry's expertise across several nodes and defeats per-pack ownership.
- **Fold the contract into ECO.** Rejected: ECO owns the generic plugin mechanism; the pack-specific rules (standards declaration, reference datasets, read-only orphan results) are IND's requirements and need one owner here.
- **Merge PATTERN into SHEET.** Rejected: pattern recognition is not sheet-specific (aerostructures, castings), and merging would make SHEET too large to split cleanly.

## Open questions

- Side-effect class of pack install/enable/disable awaits ECO's plugin node (see `IND.CONTRACT`).
- Die compensation from springback results is not covered pending a maintainer view on the out-of-scope boundary (see `IND.SHEET`).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| IND.CONTRACT | Pack contract and lifecycle | 001 002 004 005 | none |
| IND.AIRFOIL | Airfoil and blade pack | 001 002 003 004 | GOM.SPE.05, PCD.IND.01, PW.IND.01 |
| IND.GEAR | Gear pack | 001 002 003 004 | PCD.IND.02 |
| IND.SHEET | Sheet metal pack | 001 002 003 004 | GOM.SPE.01-03, PW.IND.03 |
| IND.GAPFL | Gap and flush pack | 001 002 003 004 | GOM.SPE.04, PW.IND.02 |
| IND.TUBE | Pipe and tube pack | 001 002 003 004 | PW.IND.04, PCD.IND.04 |
| IND.PATTERN | Feature-pattern pack | 001 002 004 | GOM.SPE.06 |
| IND.PORT | Portable-device workflow pack | 001 002 004 | PCD.IND.03 |
