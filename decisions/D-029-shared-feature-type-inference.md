---
id: D-029
title: Where feature-type inference from points lives
status: accepted
raised_by: IND.PORT split
blocks: [IND.PORT.RECOG]
decide_by: before IND.PORT.RECOG or CMM.STRAT.LEARN is finished as a leaf
updated: 2026-11-17
---

# D-029: Where feature-type inference from points lives

## Question

Should inferring a feature type (plane, line, circle, cylinder, sphere, point) from a small set of probed points be one shared kernel-level capability, or owned separately by each consumer?

## Context

Two nodes need the same capability:
- `IND.PORT.RECOG`: auto feature recognition from points taken with a portable arm or tracker (PCD.IND.03).
- `CMM.STRAT.LEARN`: feature-type inference from manual CMM hits, ranking candidate fits by residual and parameter count.

Both fit every candidate primitive and rank them. Owning it twice risks two ranking rules that give different answers for the same points, which users would see as inconsistent between apps. Moving it into KRN changes KRN's scope (a level-1 domain), which an agent may not do on its own (AGENTS.md rule 7).

## Options

| Option | For | Against |
|---|---|---|
| A. Shared module in KRN (for example `KRN.CONS` or a new KRN child) exposing a ranked candidate list; IND.PORT and CMM add only their UI and draft flows | One ranking rule; one test corpus; usable from scan inspection too | KRN grows; needs a KRN maintainer |
| B. Owned by `CMM.STRAT.LEARN` (earlier phase, P2) and consumed by `IND.PORT` | No KRN change; CMM ships it first | A pack depends on a CMM module; Network app inherits CMM concepts |
| C. Each consumer owns its own | No coupling | Duplicate logic and inconsistent answers |

## Recommendation

A. The inference is a pure computation over fits that KRN already owns, it belongs below every app, and the kernel's determinism rules then apply automatically.

## Decision

**Accepted (recommendation as written above), 2026-11-17, founder.**

A. The inference is a pure computation over fits that KRN already owns, it belongs below every app, and the kernel's determinism rules then apply automatically.
