---
id: GDT.LOC.COAX
title: Coaxiality and concentricity
level: 3
parent: GDT.LOC
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.LOC-004]
covers: [GOM.GDT.09]
depends_on: [GDT.LOC.DERIV, GDT.LOC.LRES, GDT.ZONE.POSE]
cli: [fid check eval-coaxiality, fid check eval-concentricity]
mcp: [check_eval_coaxiality, check_eval_concentricity]
journal: [check.eval_coaxiality@1, check.eval_concentricity@1]
updated: 2026-11-16
---

# GDT.LOC.COAX: Coaxiality and concentricity

## Purpose

Evaluates coaxiality (ISO 1101: derived axis within a cylindrical zone about a datum axis) and concentricity (centre points of cross-sections within a zone about a datum point or axis), only for the dialect and edition combinations the DMT marks supported. ASME Y14.5-2018 removed the concentricity symbol [unverified], so whether to evaluate it for legacy-edition drawings is an expert call; this module therefore depends on ISO rows (P1b) and on that call.

## Scope

**In:**
- Coaxiality from derived axes, cylindrical zone about the datum axis
- Concentricity from median points of sections, per DMT rows where supported
- `UNSUPPORTED` for combinations the DMT does not support

**Out** (and which node owns it instead):
- Derived elements (owned by `GDT.LOC.DERIV`)
- Runout (owned by `GDT.PROF.RUNOUT`)

## Requirements

- **REQ-GDT.LOC.COAX-001** The system shall evaluate coaxiality and concentricity only for dialect and edition combinations the DMT marks supported and shall return `UNSUPPORTED` otherwise.
- **REQ-GDT.LOC.COAX-002** Coaxiality shall be reported as the diameter of the smallest cylindrical zone, coaxial with the datum axis, that contains the derived axis.
- **REQ-GDT.LOC.COAX-003** Values shall match every ORS-GDT coaxiality and concentricity case within its stated tolerance.

## Interfaces

- **Offers:** `check.eval_coaxiality@1`, `check.eval_concentricity@1` (W2, dry-run R1).
- **Consumes:** derived axes and median points (GDT.LOC.DERIV); payload (GDT.LOC.LRES); pose (GDT.ZONE.POSE).

## Decomposition hints

COAXIS (coaxiality), CONC (concentricity), GATE (dialect gating via resolver output).

## Open questions

- Raised on `GDT.LOC`: whether to evaluate concentricity under ASME for legacy-edition drawings. Route to the DMT reviewers (D-009) rather than a separate decision.

## Children

_None yet: status `open`._
