---
id: ALN.DATUM.ELEM
title: Priority-ordered alignment by geometric elements
level: 3
parent: ALN.DATUM
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-ALN.DATUM-001, REQ-ALN.DATUM-004, REQ-ALN.DATUM-007]
covers: [GOM.ALN.06, PW.ALN.03]
depends_on: [ALN.DATUM.DOF, ALN.MODEL.ELEM, ALN.QA.REPORT, KRN.LSQ, KRN.SOLV, GEO.FRAME]
cli: [fid align run-elements]
mcp: [align_run_elements]
journal: [align.run_elements@1]
updated: 2026-11-16
---

# ALN.DATUM.ELEM: Priority-ordered alignment by geometric elements

## Purpose

Aligns measured data by pairing fitted measured elements with their nominals (planes, cylinders, cones, circles, spheres, lines, points) in a user-given priority order: each pair constrains the DoF its type implies, and a lower-priority pair may only move the part within the motions left free by the pairs above it (through `ALN.DATUM.DOF`'s projectors). No dialect or datum simulator is involved; that is `ALN.DATUM.SYS`.

## Scope

**In:**
- Pair input: measured element, nominal element, priority, optional per-pair weight
- Per-type pair residuals (distance and angle of the measured element to the nominal, with recorded length scaling)
- The priority-ordered solve: pairs at the same priority are fitted together; each priority level is solved within the free subspace left by the levels above
- Unconstrained-DoF handling through `ALN.DATUM.DOF`'s policy
- `align.run_elements@1`

**Out** (and which node owns it instead):
- Simultaneous unordered feature best-fit (`ALN.BFIT.FEAT`)
- Datum-system alignment under ASME or ISO (`ALN.DATUM.SYS`, `ALN.DATUM.ISO`)
- DoF accounting and projectors (`ALN.DATUM.DOF`); fitting the elements (`KRN.LSQ`)

## Requirements

- **REQ-ALN.DATUM.ELEM-001** When `align.run_elements` runs, the system shall accept any combination of supported measured/nominal pairs of matching type and report for each pair its nominal and effective constrained DoF.
- **REQ-ALN.DATUM.ELEM-002** The solve shall process priority levels from highest to lowest, and the update at each level shall be restricted to the free subspace left by higher levels, so that a lower level never changes a DoF fixed by a higher level by more than 1e-12 m or 1e-12 rad.
- **REQ-ALN.DATUM.ELEM-003** Within one priority level, the solve shall minimise the weighted sum of squared pair residuals.
- **REQ-ALN.DATUM.ELEM-004** If a measured element's type does not match its nominal's type, then the command shall fail with `pair_type_mismatch` naming the pair, and journal nothing.
- **REQ-ALN.DATUM.ELEM-005** Given noise-free synthetic element sets with a known transform, the alignment shall recover it within 1e-9 m and 1e-9 rad [unverified].
- **REQ-ALN.DATUM.ELEM-006** The solve shall run in the CPU kernel under KNP-1 and declare class D0.

## Interfaces

- **Offers:** `align.run_elements@1` (W2; params `pairs` with priority and weight, `unconstrained: keep_parent | reject`, `parent`); an R1 entry used by `ALN.DATUM.SYS` for the solve step.
- **Consumes:** `ALN.DATUM.DOF`; KRN fitted features and KRN.SOLV; GEO frames; `ALN.MODEL.ELEM`; `ALN.QA.REPORT`.

## Decomposition hints

PAIRRES (per-type pair residuals and scaling), LEVELS (priority-level solve with projectors), CMD (command). PAIRRES may share per-type deviation code with `ALN.BFIT.FEAT`; keep one implementation and make the other depend on it.

## Open questions

- GOM's and PolyWorks' element alignments may treat equal-priority pairs differently [unverified]; the same-level least-squares rule is this node's proposal.

## Children

_None yet: status `open`._
