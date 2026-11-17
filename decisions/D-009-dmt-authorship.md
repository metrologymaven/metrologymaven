---
id: D-009
title: Who authors and reviews GD&T interpretation rows (DMT)
status: accepted
raised_by: GDT split
blocks: [GDT.RES, UNC.ORS]
decide_by: before the ASME row set is frozen
updated: 2026-11-16
---

# D-009: Who authors and reviews GD&T interpretation rows (DMT)

## Question

In an open project, who writes the rows that map each GD&T callout to an evaluation method (the DMT), who reviews them, and how are they credited?

## Context

GDT.RES owns the DMT and the generated support matrix. UNC.ORS builds reference cases from it. The rows need credentialed GD&T and ISO GPS expertise. Standards text can't be pasted or sent to external LLMs, so agents can draft structure but not interpretations. The baseline budgets 150–300 h for the first ASME subset (founder-decisions A4).

The GDT agent also noted a naming mismatch: the concept docs call the table the "Dialect Mapping Table", while the GDT node calls it the "defaults-mapping table". Pick one.

## Options

| Option | For | Against |
|---|---|---|
| A. Paid expert author(s) for the first subset, then a community "DMT reviewer" role with two-expert sign-off per row | Quality from day one; a growth path | Costs money early |
| B. Community-only, with rows marked `unreviewed` until two credentialed reviewers sign off | Free | Slow; risky for a correctness-critical core |
| C. Partner with a university GD&T program or a standards-active consultancy | Credibility; students as contributors | Depends on finding a partner |

## Recommendation

A + C. Use "Dialect Mapping Table" (the concept-doc name) everywhere.

## Decision

**Accepted (options A + C), 2026-11-16, founder.** Paid expert author(s) for the first ASME subset, then a community DMT-reviewer role with two-expert sign-off per row, plus a university or consultancy partnership. The table is called the **Dialect Mapping Table (DMT)** everywhere.
