---
id: D-022
title: Die compensation from springback results
status: open
raised_by: IND.SHEET split
blocks: []
decide_by: before IND.SHEET.SPRING is finished as a leaf
updated: 2026-11-16
---

# D-022: Die compensation from springback results

## Question

Should the sheet metal pack offer die compensation, i.e. morphing the nominal CAD or die surface to counter measured springback, or is that out of scope?

## Context

`IND.SHEET` covers springback assessment (GOM.SPE.02). Some incumbents also offer compensation of die geometry from measured springback [unverified]. FID's scope puts reverse-engineering surface modelling out of scope (`seeds/out-of-scope.md`), and compensation produces new CAD geometry rather than a measurement result. `IND.SHEET.SPRING` currently excludes it; this decision makes that exclusion explicit or reverses it.

## Options

| Option | For | Against |
|---|---|---|
| A. Out of scope; add a row to `seeds/out-of-scope.md` | Keeps the pack a measurement tool; no surface-modelling engine needed | Users who expect it go elsewhere |
| B. Export only: write a per-region or per-point displacement field that external die-design tools consume | Useful hand-off without modelling surfaces | Needs an exchange format and a maintainer view on correctness |
| C. Full compensation (CAD morphing) in a separate pack | Feature parity [unverified] | Large; overlaps excluded surface modelling |

## Recommendation

B, as a later child of `IND.SHEET.SPRING`, with A as the default until a stamping maintainer volunteers. Mark incumbent behaviour `[unverified]` until checked.

## Decision

_Open._
