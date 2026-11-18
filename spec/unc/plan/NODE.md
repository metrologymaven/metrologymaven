---
id: UNC.PLAN
title: Measurability and visibility planning
level: 2
parent: UNC
kind: subsystem
status: split
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC-007]
covers: [SA.UNC.04]
depends_on: [UNC.INST, UNC.PROP, UNC.XFM, GEO]
cli: [fid unc *-measurability, fid unc *-visibility]
mcp: [unc_*_measurability, unc_*_visibility]
journal: [unc.*_measurability]
updated: 2026-11-16
---

# UNC.PLAN: Measurability and visibility planning

## Purpose

Estimates, **before execution**, whether planned measurements can be made and how well: line-of-sight visibility of targets and features from candidate instrument stations or probe directions, and the expected uncertainty of each planned characteristic from the instrument models. It serves network planning (station placement) and CMM planning (feature reachability and expected U). It is separate because it runs on plans and simulated observations, not on measured data.

## Scope

**In:**
- Visibility of points and features from a station or sensor pose against part, fixture and environment geometry
- Range and incidence-angle limits from the instrument model
- Expected per-characteristic uncertainty from simulated observations
- Ranking candidate stations or configurations by expected uncertainty
- Measurability reports for a plan

**Out** (and which node owns it instead):
- Collision checking and path planning (owned by `CMM`)
- Network adjustment of real data (owned by `LVM`)
- Instrument model definitions (owned by `UNC.INST`)
- Ray-casting geometry primitives (owned by `GEO`)

## Requirements

- **REQ-UNC.PLAN-001** When a plan and candidate station poses are given, the system shall report for each target whether it is visible, blocked, out of range or beyond the incidence-angle limit of the bound instrument model.
- **REQ-UNC.PLAN-002** Visibility shall be computed on the CPU against the declared geometry, and the result shall be deterministic for the same inputs (class D1 or better).
- **REQ-UNC.PLAN-003** When a planned characteristic has bound instrument and uncertainty models, the system shall report its expected expanded uncertainty from simulated observations, classed as a prediction and never written into a ResultRecord.
- **REQ-UNC.PLAN-004** The system shall rank candidate station sets by the largest expected uncertainty over the selected targets.
- **REQ-UNC.PLAN-005** If a planned characteristic's expected U exceeds a configured fraction of its tolerance (default 0.25 [unverified]), then the system shall flag it in the measurability report.
- **REQ-UNC.PLAN-006** A measurability evaluation for 1000 targets and 10 candidate stations shall complete in under 5 s on the reference desktop [unverified].

## Interfaces

- **Offers:** `unc.estimate_measurability@1` (R1), `unc.check_visibility@1` (R1), `unc.rank_station@1` (R1); saving a measurability report is W2 (journal `unc.*_measurability`). Consumed by `LVM` and `CMM` planning.
- **Consumes:** instrument models from `UNC.INST`; propagation from `UNC.PROP`; transform and field uncertainty from `UNC.XFM`; geometry and ray queries from `GEO`.

## Decomposition hints

VIS (visibility), PRED (expected uncertainty), RANK (station ranking), REPORT (measurability report).

## Open questions

- Whether optimising station placement (search) belongs here or in LVM is open; this node only evaluates and ranks given candidates.

## Split rationale

Split along the **evaluation pipeline**: can each target be seen (VIS), how well would each characteristic be measured (PRED), which candidate station set is best (RANK), and the end-to-end report with flags, budget and saving (REPORT). VIS, RANK and REPORT are concrete enough to be leaves; PRED depends on the open propagation, instrument-observation and transform-uncertainty modules and stays open.

Alternatives considered:
- **Network planning and CMM planning as separate children.** Rejected: both use the same visibility and prediction steps; only the instrument model and the target source differ.
- **Station search (optimisation) as a child.** Not added: the parent's open question leaves search unassigned between UNC and LVM; RANK evaluates given candidates only.

Note: the children's commands `unc.check_visibility@1`, `unc.predict_uncertainty@1` and `unc.rank_station@1` fall outside this node's frontmatter `cli`/`journal` patterns (`*-measurability`, `*-visibility`), which were narrower than its own Interfaces list; reviewers may widen the patterns.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| UNC.PLAN.VIS | Visibility, range and incidence checks for planned targets (leaf) | 001 002 | SA.UNC.04 |
| UNC.PLAN.PRED | Expected uncertainty from simulated observations | 003 | SA.UNC.04 |
| UNC.PLAN.RANK | Ranking candidate station sets (leaf) | 004 | SA.UNC.04 |
| UNC.PLAN.REPORT | Measurability evaluation and report (leaf) | 005 006 | SA.UNC.04 |
