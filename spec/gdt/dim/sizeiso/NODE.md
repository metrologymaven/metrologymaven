---
id: GDT.DIM.SIZEISO
title: ISO 14405-1 size modifiers
level: 3
parent: GDT.DIM
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.DIM-001, REQ-GDT.DIM-006, REQ-GDT.DIM-007]
covers: []
depends_on: [GDT.DIM.SIZEASME, GDT.RES.RESOLVE, KRN.ENV, KRN.LSQ, KRN.MZ]
uses_artifacts: [GDT.RES.ROWISO, UNC.ORS.SIZE]
cli: [fid check eval-size]
mcp: [check_eval_size]
journal: [check.eval_size@1]
updated: 2026-11-17
---

# GDT.DIM.SIZEISO: ISO 14405-1 size modifiers

## Purpose

Evaluates ISO 14405-1 sizes: local sizes (two-point LP, spherical LS), global sizes (least-squares GG, maximum inscribed GX, minimum circumscribed GN, minimax GC), calculated sizes (CC, CA, CV) and rank-order modifiers (SX, SN, SA, SM, SD, SR, SQ), tagged per edition (2016 legacy and 2025). It plugs into the `check.eval_size@1` command owned by `GDT.DIM.SIZEASME` as the ISO dispatch path.

## Scope

**In:**
- Each modifier as an operator chain from an ISO DMT row, per edition
- Rank-order statistics over a local-size distribution
- Edition tag per result; refusal of modifiers the DMT marks unsupported for that edition

**Out** (and which node owns it instead):
- The size command and ASME path (owned by `GDT.DIM.SIZEASME`)
- Fits (owned by `KRN`)
- ISO rows (owned by `GDT.RES.ROWISO`)

## Requirements

- **REQ-GDT.DIM.SIZEISO-001** When an ISO 14405-1 size modifier is evaluated, the system shall tag the result with the ISO 14405-1 edition from the governing spec.
- **REQ-GDT.DIM.SIZEISO-002** If the DMT marks a modifier unsupported for that edition, then the system shall return `UNSUPPORTED` and compute nothing.
- **REQ-GDT.DIM.SIZEISO-003** Rank-order modifiers shall be computed over the local sizes produced by the chain's local-size operator, with the rank statistic's definition taken from the row.
- **REQ-GDT.DIM.SIZEISO-004** Values shall match every ORS-SIZE ISO case within its stated tolerance.

## Interfaces

- **Offers:** the ISO dispatch path of `check.eval_size@1`.
- **Consumes:** fits (KRN); ISO rows (GDT.RES.ROWISO); ORS-SIZE (UNC.ORS, as data).

## Decomposition hints

LOCAL (LP, LS), GLOBAL (GG, GX, GN, GC), CALC (CC, CA, CV), RANK (rank-order statistics).

## Open questions

- ISO 14405-1:2025 reportedly narrowed its scope [unverified]; the edition split must come from the purchased text (D-016). Local and calculated size definitions are therefore kept in open children (`GDT.DIM.SIZEISO.LOCAL`, `GDT.DIM.SIZEISO.CALC`); dispatch, global sizes from a named association and rank-order statistics are mechanics and are leaves.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| GDT.DIM.SIZEISO.DISP | ISO size evaluator, edition tag and unsupported refusal (leaf) | 001 002 | none |
| GDT.DIM.SIZEISO.GLOBAL | Global sizes from a named association (leaf) | 004 | none |
| GDT.DIM.SIZEISO.RANK | Rank-order statistics over local sizes (leaf) | 003 004 | none |
| GDT.DIM.SIZEISO.LOCAL | ISO local sizes (two-point and spherical) | 004 | none |
| GDT.DIM.SIZEISO.CALC | ISO calculated sizes (circumference, area, volume) | 004 | none |
