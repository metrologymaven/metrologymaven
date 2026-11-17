---
id: D-021
title: Publishing benchmark comparisons with incumbent products
status: open
raised_by: OUTR.BENCH split
blocks: [OUTR.BENCH.CMP]
decide_by: before the first comparison is published
updated: 2026-11-16
---

# D-021: Publishing benchmark comparisons with incumbent products

## Question

Will the project publish measured comparisons between its results or performance and those of named incumbent products, and under what conditions?

## Context

REQ-OUTR.BENCH-005 allows comparisons only when they state the incumbent's version and settings and can be reproduced by a third party who has the product. Some commercial licence agreements restrict publishing benchmark results of the licensed software [unverified for each incumbent]. Running an incumbent to produce numbers also needs a lawful licence held by someone willing to be named or to stay anonymous. A published comparison is a strong public claim and a legal exposure, so the choice belongs to maintainers and counsel. OUTR.BENCH.CMP cannot be specified without it. The other benchmark children compare against reference data, not incumbents, and are not blocked.

## Options

| Option | For | Against |
|---|---|---|
| A. No incumbent comparisons; publish only results against public reference data | No licence risk; simplest | Users cannot see a direct comparison |
| B. Comparisons only from third parties (users or labs) who hold licences, published as their signed statement with our reproducible half | Licence holder takes responsibility for their own terms | Depends on volunteers; harder to keep current |
| C. Project-run comparisons after a counsel review of each incumbent's licence terms | Most control and consistency | Needs a licence and counsel per incumbent; may be forbidden |

## Recommendation

A for the first releases, then B once there are users willing to publish. Counsel review is needed before any option B or C publication. [unverified: actual licence terms]

## Decision

_Open._ When decided: OUTR.BENCH.CMP returns to `open` (B or C) or is retired with a change to REQ-OUTR.BENCH-005 through the steering group (A).
