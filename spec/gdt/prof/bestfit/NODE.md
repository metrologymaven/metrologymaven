---
id: GDT.PROF.BESTFIT
title: Profile best-fit options
level: 3
parent: GDT.PROF
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.PROF-003]
covers: [PCD.DIM.06]
depends_on: [GDT.PROF.SURF, GDT.ZONE.POSE]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GDT.PROF.BESTFIT: Profile best-fit options

## Purpose

Implements the best-fit options incumbents offer on profile checks (none, free best fit within the zone, constrained to named DRF DOFs, and with datum mobility through `GDT.ZONE.MOBSRCH`), and records the chosen option and resulting transform. Which options are legitimate for a given callout is an interpretation question; unconstrained best fit on a datum-referenced profile can contradict the drawing, so each option maps to a DMT row variant and non-default choices are flagged.

## Scope

**In:**
- Options: `none`, `free`, `constrained{dofs}`, `mobility`
- Objective: minimum zone usage (default) or least squares where the variant says so
- Recording option, objective, DOFs freed and transform
- Flag `bestfit_beyond_drf` when the option frees DOFs the DRF constrains

**Out** (and which node owns it instead):
- Pose solver (owned by `GDT.ZONE.POSE`)
- Mobility search (owned by `GDT.ZONE.MOBSRCH`)
- Alignment best fits (owned by `ALN.BFIT`)

## Requirements

- **REQ-GDT.PROF.BESTFIT-001** When a profile check has a best-fit option, the system shall evaluate with the selected option (none, free, constrained to named DRF DOFs, or mobility) and record the option and the resulting transform in the ResultRecord.
- **REQ-GDT.PROF.BESTFIT-002** If the option frees DOFs that the characteristic's DRF constrains, then the result shall carry `bestfit_beyond_drf` and list the freed DOFs.
- **REQ-GDT.PROF.BESTFIT-003** Results shall match every ORS-GDT profile best-fit case within its stated tolerance.

## Interfaces

- **Offers:** best-fit option handling inside `check.eval_surface_profile@1` and `check.eval_line_profile@1`.
- **Consumes:** pose solve; surface-profile problem (GDT.PROF.SURF).

## Decomposition hints

OPTS (option model and flags; a leaf), OBJ (objective variants).

## Open questions

- Whether `free` best fit on a datum-referenced profile should be allowed at all, or only flagged; an expert call through the DMT.

## Children

_None yet: status `open`._
