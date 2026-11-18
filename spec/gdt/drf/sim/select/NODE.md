---
id: GDT.DRF.SIM.SELECT
title: Applying a DMT selection rule to rocking candidates
level: 4
parent: GDT.DRF.SIM
kind: feature
status: open
phase: P1a
size: L
owners: []
claimed_by:
needs_expert: gdt
satisfies: [REQ-GDT.DRF.SIM-002, REQ-GDT.DRF.SIM-004]
covers: []
depends_on: [GDT.DRF.SIM.ROCK, GDT.RES.RESOLVE]
uses_artifacts: [GDT.RES.ROWASME, GDT.RES.ROWISO, UNC.ORS.DRF]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.DRF.SIM.SELECT: Applying a DMT selection rule to rocking candidates

## Purpose

Chooses one candidate datum set from a rocking candidate list using the selection rule the resolved DMT row names, and records every candidate together with the chosen one. The rules (for example minimising a secondary objective, or a rule that depends on the next datum in precedence) are interpretation content that differs between ASME Y14.5-2018 and the ISO 5459 editions [unverified]; they come only from signed-off rows (D-009, D-016). This node implements the rules those rows name and refuses any other.

## Scope

**In:**
- Executing the selection rule named by the chain over a `CandidateSet`
- Recording all candidates, the chosen index and the rule's algorithm ID
- Refusal of rules not implemented

**Out** (and which node owns it instead):
- Candidate filtering (owned by `GDT.DRF.SIM.ROCK`)
- The rules as content (owned by DMT rows via `GDT.RES`)

## Requirements

- **REQ-GDT.DRF.SIM.SELECT-001** When the chain names a selection rule, the system shall choose exactly one candidate by that rule and record every candidate, the chosen index and the rule's algorithm ID.
- **REQ-GDT.DRF.SIM.SELECT-002** If the chain names a rule that is not implemented, then the system shall return `UNSUPPORTED` with the rule ID and no simulated datum.
- **REQ-GDT.DRF.SIM.SELECT-003** Selections shall match every ORS-DRF rocking case within its stated tolerance, including cases where minimax and constrained L2 simulators disagree.

## Interfaces

- **Offers:** `select(candidates, chain, context) -> SimulatedDatum` to `GDT.DRF.BUILD`.
- **Consumes:** candidate sets (GDT.DRF.SIM.ROCK); chain (GDT.RES.RESOLVE); rows and ORS-DRF cases (read as data).

## Decomposition hints

One leaf per rule family that signed-off rows name; a test-only rule (lowest objective, then lowest contact indices) may exist behind a test feature flag but must never be reachable from a released DMT.

## Open questions

- Not a leaf: selection rules are edition-specific DMT content (D-009; texts per D-016).

## Children

_None yet: status `open`._
