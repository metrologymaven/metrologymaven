---
id: GDT.DRF.SIM
title: Datum-feature simulators and rocking candidate sets
level: 3
parent: GDT.DRF
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.DRF-001, REQ-GDT.DRF-005, REQ-GDT.DRF-007]
covers: [GOM.GDT.14]
depends_on: [KRN.ENV, GDT.RES.RESOLVE]
uses_artifacts: [GDT.RES.ROWASME, GDT.RES.ROWISO, UNC.ORS.DRF]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.DRF.SIM: Datum-feature simulators and rocking candidate sets

## Purpose

Turns a measured datum feature into a simulated datum (plane or axis) using the simulator the resolved DMT row names: external minimax, constrained L2 or L1 outer-tangent plane, line or cylinder (fits from `KRN.ENV`), respecting the orientation and location constraints imposed by higher-precedence datums. When a primary planar feature rocks (several candidate datum sets within the simulator's tie tolerance), it enumerates the candidates, applies the row's selection rule and records all candidates. Which simulator and which selection rule apply is DMT content per edition (ASME Y14.5-2018, ISO 5459:2011, ISO 5459:2024 differ [unverified]).

## Scope

**In:**
- Invoking the named KRN tangent or constrained fit with precedence constraints (for example secondary perpendicular to primary)
- Candidate-set enumeration for rocking planar and cylindrical datum features within a stated tie tolerance
- Applying the DMT row's selection rule and recording candidates and the chosen set
- Output: `SimulatedDatum` plus simulator algorithm ID and certificate

**Out** (and which node owns it instead):
- The fits themselves (owned by `KRN.ENV`)
- DRF assembly and DOF bookkeeping (owned by `GDT.DRF.BUILD`)
- Datum targets and common datums (owned by `GDT.DRF.COMMON`)

## Requirements

- **REQ-GDT.DRF.SIM-001** The simulator shall use exactly the fit the resolved DMT row names, with the precedence constraints from higher datums, and shall record its algorithm ID and certificate.
- **REQ-GDT.DRF.SIM-002** When more than one candidate datum set lies within the row's tie tolerance, the simulator shall enumerate all candidates up to a stated cap, apply the row's selection rule, and record every candidate and the chosen one.
- **REQ-GDT.DRF.SIM-003** If the candidate count exceeds the cap, then the simulator shall return `rocking_unbounded` and no simulated datum.
- **REQ-GDT.DRF.SIM-004** The simulators shall reproduce every ORS-DRF case, including rocking cases where minimax and constrained L2 disagree, within the case's stated tolerance.

## Interfaces

- **Offers:** `simulate(datum_feature, row, constraints) -> SimulatedDatum + candidates` used by `GDT.DRF.BUILD`.
- **Consumes:** tangent and constrained fits (KRN.ENV); DMT rows (GDT.RES.RESOLVE).

## Decomposition hints

FIT (single-candidate simulation with precedence constraints; likely a leaf), ROCK (candidate enumeration on the hull facets), SELECT (applying a row's selection rule).

## Open questions

- The default planar simulator and the rocking selection rule are edition-specific DMT content (D-009 authorship; texts per D-016). Resolved at split: executing a named fit (`GDT.DRF.SIM.FIT`) and filtering candidates (`GDT.DRF.SIM.ROCK`) are mechanics and are leaves; applying a selection rule (`GDT.DRF.SIM.SELECT`) stays open.
- DoF table: the fixed DoFs come from the single table owned by `GDT.DRF.BUILD`. The R1 entry for `ALN.DATUM.SYS` is the dry-run of `gdt.build_drf@1` (owned by `GDT.DRF.BUILD`), which calls these simulators; no simulator command is exposed separately.
- `KRN.ENV.TAN` must accept orientation and location constraints from higher datums (raised in `GDT.DRF.SIM.FIT`).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| GDT.DRF.SIM.FIT | Single-feature datum simulation with precedence constraints (leaf) | 001 004 | GOM.GDT.14 |
| GDT.DRF.SIM.ROCK | Rocking candidate sets within a tie tolerance (leaf) | 002 003 004 | none |
| GDT.DRF.SIM.SELECT | Applying a DMT selection rule to rocking candidates | 002 004 | none |
