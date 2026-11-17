---
id: D-018
title: Phase tags for CMM level-3 children later than P2
status: accepted
raised_by: CMM level-2 expansion
blocks: []
decide_by: before CMM.GEN.PMI or CMM.COLL.ROT is expanded
updated: 2026-11-17
---

# D-018: Phase tags for CMM level-3 children later than P2

## Question

Should the CMM level-3 children listed below carry a phase later than their P2 parents, as the Design Baseline §8 and the HAL concept doc §6.9 suggest?

## Context

All seven CMM subsystems are P2. When they were split, a few children were tagged later than P2 because the concept docs schedule that work later. A phase later than the parent is allowed by CONSTITUTION §7 (D-006: a phase means "work starts"), but the `CMM.GEN`, `CMM.PATH` and `CMM.COLL` open questions say a phase change needs a decision, so the tags are proposed here for ratification. No node is blocked: the children stay `open` with the proposed tag, and the tag is reverted to P2 if this is rejected.

| Node | Proposed phase | Source of the proposal |
|---|---|---|
| `CMM.GEN.PMI` (programs from PMI) | P3 | Design Baseline §8 and HAL concept §6.9 put auto-path from PMI in P3 |
| `CMM.COLL.ROT` (rotary tables) | P3 | Concept docs put rotary and articulating kinematics beyond 3-axis Cartesian in P3 |
| Future grandchild `CMM.GEN.DMIS.EXPORT` (Command Program to DMIS) | later | `IO.DMIS` schedules its program writer (`export.dmis_program@1`) as `later` |
| Future grandchild of `CMM.PATH` for a graphical path editor, if one is made | P3 | Design Baseline §8 |

Kept at P2 on purpose (not moved): `CMM.PATH.AUTO` (auto path from a selected feature list is the P2 "strategy-to-path generator"), `CMM.PROBE.HEAD` (indexing heads are needed for P2 qualification; only dynamic continuous articulation is deferred inside it).

## Options

| Option | For | Against |
|---|---|---|
| A. Ratify the tags as proposed | Matches the concept docs' scheduling; keeps P2 small | Rotary tables are a common prismatic need and may be missed in P2 |
| B. Keep everything at P2 | One phase for the whole domain | P2 is already the largest phase (doc 08 §10 estimates 8-12 founder-months [unverified]) |
| C. Ratify, but keep `CMM.COLL.ROT` at P2 | Rotary indexing (not continuous) is cheap to model | Rotary-aware path planning and simulation would still be P3 |

## Recommendation

A. Revisit `CMM.COLL.ROT` (option C) if a P2 partner machine has a rotary table.

## Decision

**Accepted (recommendation as written above), 2026-11-17, founder.**

A. Revisit `CMM.COLL.ROT` (option C) if a P2 partner machine has a rotary table.
