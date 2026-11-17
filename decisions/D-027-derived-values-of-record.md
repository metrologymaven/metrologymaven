---
id: D-027
title: Derived values of record from user arithmetic
status: accepted
raised_by: AUTO.PROG and AUTO.SCRIPTED level-3 expansion
blocks: []
decide_by: before AUTO.SCRIPTED.RESULT is split
updated: 2026-11-17
---

# D-027: Derived values of record from user arithmetic

## Question

Should the kernel offer a deterministic expression command (KNP-1, D0) so that programs and scripted checks can produce values of record from arithmetic on other results (for example the ratio of two distances, or the mean of four diameters)?

## Context

CONSTITUTION C1: every value of record is computed by the deterministic kernel. REQ-AUTO.PROG-005 therefore keeps program expressions out of values of record, and REQ-AUTO.SCRIPTED-004 refuses any scripted-check value that no kernel command produced. Without a kernel expression command, a user who needs "distance A / distance B" as a reported characteristic has no path except a plugin.

Incumbent tools let users report computed characteristics from formulas [unverified for each incumbent]. Seeds GOM.AUT.04 (scripted checks) and PCD.PRG.02 (variables and expressions) touch this.

Affects: `AUTO.SCRIPTED.RESULT`, `AUTO.PROG.EXPR`, and a new command in `KRN` (outside AUTO, so AUTO cannot add it).

## Options

| Option | For | Against |
|---|---|---|
| A. KRN adds `expr.evaluate_formula@1`: a small typed, unit-checked expression language evaluated under KNP-1 (D0), with uncertainty status UEC-0 unless UNC propagates it | Keeps C1; one evaluator for programs and scripted checks; replayable | New kernel surface; uncertainty propagation through formulas needs UNC work |
| B. No derived values of record; formulas only for display and control flow | Smallest kernel | Users fall back to plugins or external tools; parity gap |
| C. Allow program expressions (outside the kernel) to produce values of record if evaluated with the kernel's numeric facade | Fast to build | Blurs C1; two evaluators to keep bit-identical |

## Recommendation

A. The expression language and its checker can be shared with `AUTO.PROG.EXPR` (AUTO owns syntax and typing, KRN owns evaluation for values of record). UNC decides whether formula results start at UEC-0.

## Decision

**Accepted (recommendation as written above), 2026-11-17, founder.**

A. The expression language and its checker can be shared with `AUTO.PROG.EXPR` (AUTO owns syntax and typing, KRN owns evaluation for values of record). UNC decides whether formula results start at UEC-0.
