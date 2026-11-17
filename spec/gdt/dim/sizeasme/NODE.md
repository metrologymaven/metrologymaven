---
id: GDT.DIM.SIZEASME
title: ASME feature-of-size: two-point size and Rule #1 envelope
level: 3
parent: GDT.DIM
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.DIM-001, REQ-GDT.DIM-005, REQ-GDT.DIM-007]
covers: [PW.GDT.01]
depends_on: [GDT.CHAR.SPEC, GDT.CHAR.RECORD, GDT.RES.RESOLVE, KRN.ENV]
cli: [fid check eval-size]
mcp: [check_eval_size]
journal: [check.eval_size@1]
updated: 2026-11-16
---

# GDT.DIM.SIZEASME: ASME feature-of-size: two-point size and Rule #1 envelope

## Purpose

Evaluates a feature of size under ASME Y14.5-2018: the minimum and maximum two-point local sizes and the Rule #1 envelope check against the actual mating envelope (KRN minimum-circumscribed or maximum-inscribed fit). How local sizes are paired, which cross-sections are used, and how the envelope is formed are interpretation choices that come from expert-authored DMT rows (D-009); this node executes them and reports both quantities.

## Scope

**In:**
- Two-point local size extraction per the chain (opposite-point pairing rule, sections), for cylinders, spheres and two parallel opposite planes
- Actual mating envelope from KRN envelope fits, with `local_optimum_only` propagated
- Rule #1 envelope containment at MMC and the reported envelope size
- Payload: min and max local size, envelope size, containment result, flags

**Out** (and which node owns it instead):
- Envelope fits (owned by `KRN.ENV`)
- ISO 14405-1 size modifiers (owned by `GDT.DIM.SIZEISO`)
- Bonus tolerance from actual size (owned by `GDT.ZONE.MATL`)
- Pairing rules and defaults (owned by DMT rows in `GDT.RES.ROWASME`)

## Requirements

- **REQ-GDT.DIM.SIZEASME-001** When an ASME Y14.5-2018 feature of size is evaluated, the system shall report the minimum and maximum two-point local sizes computed with the pairing rule named in the resolved chain.
- **REQ-GDT.DIM.SIZEASME-002** The system shall report the actual mating envelope size from the KRN fit named in the chain and whether the surface lies within the MMC envelope boundary.
- **REQ-GDT.DIM.SIZEASME-003** If KRN flags the envelope fit `local_optimum_only`, then the result shall carry that flag and shall not claim a global envelope.
- **REQ-GDT.DIM.SIZEASME-004** Values shall match every ORS-SIZE ASME case within its stated tolerance, including sparse and partial-arc cases where an unconstrained inscribed fit escapes.

## Interfaces

- **Offers:** `check.eval_size@1` for ASME features of size (W2, dry-run R1); actual size for `GDT.ZONE.MATL`.
- **Consumes:** envelope fits (KRN.ENV); chain (GDT.RES.RESOLVE); builder (GDT.CHAR.RECORD); ORS-SIZE cases (UNC.ORS, as data).

## Decomposition hints

LOCAL (two-point local size given a pairing rule; a leaf once rows exist), ENVLP (envelope containment and size), CMD (command and payload).

## Open questions

- Not a leaf: the pairing rule and envelope definition are DMT content awaiting expert rows (`GDT.RES.ROWASME`, blocked on D-016).
- `check.eval_size@1` is shared with `GDT.DIM.SIZEISO`; the command is owned here and dispatches by dialect.

## Children

_None yet: status `open`._
