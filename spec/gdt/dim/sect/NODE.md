---
id: GDT.DIM.SECT
title: Checks on section elements
level: 3
parent: GDT.DIM
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.DIM-001, REQ-GDT.DIM-002, REQ-GDT.DIM-007]
covers: [GOM.DIM.07]
depends_on: [GDT.DIM.LINEAR, GDT.DIM.ANGLE, GDT.DIM.RADIAL]
uses_artifacts: [SCAN.SECT]
cli: [fid check eval-section-distance, fid check eval-section-angle, fid check eval-section-radius]
mcp: [check_eval_section_distance, check_eval_section_angle, check_eval_section_radius]
journal: [check.eval_section_distance@1, check.eval_section_angle@1, check.eval_section_radius@1]
updated: 2026-11-17
---

# GDT.DIM.SECT: Checks on section elements

## Purpose

Applies distance, angle, radius and similar dimensional checks to 2D elements extracted from a section curve (a planar cut of a mesh, cloud or CAD body supplied by SCAN). It is separate from the 3D checks because section elements live in a section plane frame, carry a provenance link to the section, and only exist once SCAN sections exist (P1b), which resolves the phase question raised on `GDT.DIM`.

## Scope

**In:**
- 2D element extraction from a section curve by the operator chain (line, arc and point elements on a named curve span)
- In-plane distance, angle and radius checks on those elements, reusing the 3D computations restricted to the section plane
- Provenance: section ID, plane, curve span

**Out** (and which node owns it instead):
- Computing the section curve (owned by `SCAN.SECT`)
- 3D distance, angle and radius (owned by `GDT.DIM.LINEAR`, `GDT.DIM.ANGLE`, `GDT.DIM.RADIAL`)
- Airfoil section parameters (owned by `IND.AIRFOIL`)

## Requirements

- **REQ-GDT.DIM.SECT-001** The system shall evaluate distance, angle and radius checks on 2D elements fitted to named spans of a section curve and report values in the section plane frame.
- **REQ-GDT.DIM.SECT-002** Each section result shall record the section ID, section plane and curve span used, so that a re-cut section invalidates the result.
- **REQ-GDT.DIM.SECT-003** If the referenced section has been recomputed since the characteristic was evaluated, then the result shall be marked stale and re-evaluation shall be required before release.

## Interfaces

- **Offers:** the three section commands above.
- **Consumes:** section curves (SCAN.SECT); 2D fits (KRN); 3D check computations (siblings); ResultRecord builder (GDT.CHAR.RECORD).

## Decomposition hints

SPAN (element extraction from a curve span), CMDS (the three commands), STALE (staleness via the dependency graph, CORE.DEP).

## Open questions

- Resolved at split (D-034): the section curve is read as an artifact (`uses_artifacts: [SCAN.SECT]` on `GDT.DIM.SECT.SPAN`), not a code dependency, because SCAN is a later L2 domain than GDT.
- Resolved at split: section checks are separate commands (`GDT.DIM.SECT.CMDS`) that reuse the 3D computations, so the P1a 3D leaves stay unchanged.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| GDT.DIM.SECT.SPAN | 2D element extraction from a section-curve span (leaf) | 001 002 | GOM.DIM.07 |
| GDT.DIM.SECT.CMDS | Section distance, angle and radius commands (leaf) | 001 002 | GOM.DIM.07 |
| GDT.DIM.SECT.STALE | Staleness of section results after a re-cut (leaf) | 002 003 | none |
