---
id: OUTR.LAUNCH.GATE
title: Milestone gate check and report
level: 3
parent: OUTR.LAUNCH
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.LAUNCH-003, REQ-OUTR.LAUNCH-004]
covers: []
depends_on: [OUTR.LAUNCH.PLAN, ECO.PARITY]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.LAUNCH.GATE: Milestone gate check and report

## Purpose

The check that runs when a release candidate is tagged and states, for each criterion of the targeted milestone, whether it is met, not met or needs sign-off, from generated data only. It also blocks announcing a milestone whose criteria are not all met. It is separate from the plan because criteria are evaluated, not asserted.

## Scope

**In:**
- Evaluator for each criterion kind of the criteria file (node status, phase, parity threshold, sign-off)
- Sign-off records (who signed, role, date) checked against the roles named in the plan
- The gate report per release candidate and milestone
- The announcement block: a result the announcement process must check

**Out** (and which node owns it instead):
- Criteria content and file format (owned by `OUTR.LAUNCH.PLAN`)
- Release candidates and parity reports (owned by `ECO.REL` and `ECO.PARITY`)
- Posting announcements (owned by `OUTR.LAUNCH.ANN`)

## Requirements

- **REQ-OUTR.LAUNCH.GATE-001** When a release candidate is tagged for a milestone, the gate check shall produce a report listing every criterion of that milestone as met, not met or needs sign-off, with the evidence (node ID and status, parity value, or sign-off record) for each.
- **REQ-OUTR.LAUNCH.GATE-002** The gate check shall evaluate criteria only from the spec tree at the candidate's revision, the candidate's parity report and committed sign-off records, and shall make no network request.
- **REQ-OUTR.LAUNCH.GATE-003** If any criterion is not met or lacks a valid sign-off, then the gate report's overall result shall be "not ready" and the announcement process shall refuse to use the milestone's name.
- **REQ-OUTR.LAUNCH.GATE-004** A sign-off shall count only if the signer holds the role the criterion names in the governance roster and is not the release candidate's author.
- **REQ-OUTR.LAUNCH.GATE-005** Running the gate check twice on the same inputs shall produce byte-identical reports.

## Interfaces

- **Offers:** the gate report (published with the release candidate); the overall ready or not-ready result for OUTR.LAUNCH.ANN.
- **Consumes:** OUTR.LAUNCH.PLAN criteria file; spec tree; ECO.PARITY report; governance roster (D-003); sign-off records.

## Decomposition hints

EVAL (criterion evaluators), SIGN (sign-off records and checks), REPORT (report format and overall result).

## Open questions

- Becomes a leaf candidate once OUTR.LAUNCH.PLAN fixes the criteria file format.

## Children

_None yet: status `open`._
