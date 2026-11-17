---
id: GDT.LOC.SYM
title: Symmetry
level: 3
parent: GDT.LOC
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.LOC-004]
covers: [GOM.GDT.10]
depends_on: [GDT.LOC.DERIV, GDT.LOC.LRES, GDT.ZONE.POSE]
cli: [fid check eval-symmetry]
mcp: [check_eval_symmetry]
journal: [check.eval_symmetry@1]
updated: 2026-11-16
---

# GDT.LOC.SYM: Symmetry

## Purpose

Evaluates symmetry: median points or the derived median surface of a feature must lie within a zone of two parallel planes centred on a datum plane. It is an ISO GPS tolerance (P1b rows); its ASME status follows the DMT, as for concentricity.

## Scope

**In:**
- Median points or median surface from `GDT.LOC.DERIV`
- Two-parallel-planes zone centred on the datum median plane
- `UNSUPPORTED` for unsupported dialect and edition combinations

**Out** (and which node owns it instead):
- Derived elements (owned by `GDT.LOC.DERIV`)
- Position used as a symmetry control (owned by `GDT.LOC.POS`)

## Requirements

- **REQ-GDT.LOC.SYM-001** The system shall evaluate symmetry only for dialect and edition combinations the DMT marks supported and shall return `UNSUPPORTED` otherwise.
- **REQ-GDT.LOC.SYM-002** The symmetry value shall be the width of the smallest zone of two parallel planes, centred on the datum plane, containing the derived elements named by the chain.
- **REQ-GDT.LOC.SYM-003** Values shall match every ORS-GDT symmetry case within its stated tolerance.

## Interfaces

- **Offers:** `check.eval_symmetry@1` (W2, dry-run R1).
- **Consumes:** derived elements; payload; pose solve.

## Decomposition hints

One module; probably a single leaf once ISO rows exist.

## Open questions

- Same ASME question as `GDT.LOC.COAX`.

## Children

_None yet: status `open`._
