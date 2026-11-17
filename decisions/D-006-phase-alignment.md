---
id: D-006
title: Phase alignment after the level-1 split
status: accepted
raised_by: level check L2
blocks: [HAL, IO, UNC]
decide_by: before level-2 expansion
updated: 2026-11-16
---

# D-006: Phase alignment after the level-1 split

## Question

1. What does a node's `phase:` mean?
2. Should three domains move earlier because some of their subsystems must start earlier?

## Context

The level-1 agents tagged 44 of the 151 subsystems with a phase that differs from their domain's. In most cases the child is later than the parent, which is fine: a domain starts with its first subsystem.

In 7 cases the child is **earlier** than its domain. `scope.py` now warns about these:

| Child | Child phase | Domain phase | Why it is early |
|---|---|---|---|
| HAL.IDL | P0 | P2 | Device contracts are needed before drivers |
| HAL.SIM | P0 | P2 | The I++ simulator is a P0 test asset (seeds DEV.SIM.01) |
| HAL.ACQ | P1a | P2 | Gauge and manual-entry acquisition feed P1a reports |
| IO.REG | P0 | P1a | Safe-read framework under every format |
| IO.QIF | P0 | P1a | QIF codegen is in the baseline's P0 scope |
| IO.CORP | P0 | P1a | Corpora and fuzzing start with the first reader |
| UNC.RDL | P0 | P1a | Reference data validates the P0 kernel fits |

The agents also flagged later-phase candidates that are *not* moves, for example GDT.PROF and GDT.PMI in P1b, GEO.CAD in P1b, VIZ.WEB and VIZ.KIOSK in P1b, and LVM's offline adjustment, which could move earlier than P3.

## Options

| Option | For | Against |
|---|---|---|
| A. A phase means "work starts". Parent = earliest child. Move HAL → P0, IO → P0, UNC → P0. Accept the 37 later-child tags as proposed | Consistent; the validator enforces it | P0 looks bigger on paper (scope is unchanged) |
| B. A phase means "the domain is usable". Keep parents; allow earlier children | No parent changes | The phase tag loses its scheduling meaning |

## Recommendation

A. Also open a separate decision later on moving LVM.STATION and LVM.ADJ (offline network adjustment) to P1b. It needs no hardware and would be an early differentiator against SpatialAnalyzer.

## Decision

**Accepted (option A), 2026-11-16, founder.** `phase:` means *work starts*; a parent's phase is the earliest phase of its children, and scope.py warns otherwise. HAL, IO and UNC move to P0. The 37 later-child tags from the level-1 split are accepted as proposed. Follow-ups: CONSTITUTION §7 amended; a separate decision may later move LVM offline adjustment (LVM.STATION, LVM.ADJ) to P1b.
