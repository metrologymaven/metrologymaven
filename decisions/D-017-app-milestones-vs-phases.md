---
id: D-017
title: How app milestones relate to phases
status: open
raised_by: OUTR.LAUNCH split
blocks: [OUTR.LAUNCH.PLAN]
decide_by: before the first alpha announcement
updated: 2026-11-16
---

# D-017: How app milestones relate to phases

## Question

May a focused app be announced as 1.0 (or beta) before the phases that hold its core workflow have started or finished? In particular, may Probe or Network reach 1.0 before P2 (live devices, CMM programming) or P3 (large-volume networks)?

## Context

OUTR.LAUNCH must define alpha, beta and 1.0 per focused app (D-004) with criteria tied to phase gates (REQ-OUTR-004). Phases are when work starts (D-006), not when an app is usable. Inspect and Report can be useful from P1a/P1b. Probe's main job (programming and executing CMM measurements) depends on P2, and Network's on P3, but offline subsets (program editing with the simulator, offline network adjustment) could be released earlier. Naming milestones is a project-wide promise to users, so it belongs to maintainers. OUTR.LAUNCH.PLAN cannot write per-app criteria without this choice.

## Options

| Option | For | Against |
|---|---|---|
| A. An app's 1.0 requires every phase that holds its core workflow to be finished (Probe after P2, Network after P3); earlier releases stay alpha or beta | Simple; the name means the whole job works | Probe and Network stay pre-1.0 for a long time |
| B. Each app defines a scoped 1.0 (for example "Probe 1.0, offline programming and simulation") that is stated in the milestone name and on the app page | Earlier stable releases; honest if the scope is shown | Users may read "1.0" as the full job despite the label |
| C. Milestones are project-wide, not per app (one alpha, beta, 1.0 for the family) | One message | Conflicts with D-004's per-app release pace |

## Recommendation

A, with offline subsets shipped as beta and labelled by scope. It is the option least likely to overstate status, in line with the honest-status rule (CONSTITUTION §8). [unverified that users accept long pre-1.0 periods]

## Decision

_Open._ When decided: OUTR.LAUNCH.PLAN returns to `open` and writes per-app criteria accordingly.
