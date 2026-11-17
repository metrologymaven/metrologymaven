---
id: OUTR.COMM.REPORT
title: Quarterly adoption report
level: 3
parent: OUTR.COMM
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.COMM-006]
covers: []
depends_on: [OUTR.COMM.METRIC, OUTR.COMM.SURVEY]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.COMM.REPORT: Quarterly adoption report

## Purpose

The public quarterly adoption report: every defined metric for the quarter and the three before it, survey aggregates when a survey ran, moderation counts, and launch milestones as annotations. It is separate because it is the single public statement about adoption and must be generated from stored data, on a fixed schedule.

## Scope

**In:**
- Report generator from METRIC snapshots, SURVEY aggregates and MOD counts
- Report layout with metric definitions linked
- Milestone annotations from the launch plan
- Publication on the website within the deadline

**Out** (and which node owns it instead):
- Metric collection and definitions (owned by `OUTR.COMM.METRIC`)
- Survey aggregation (owned by `OUTR.COMM.SURVEY`)
- Moderation counts (owned by `OUTR.COMM.MOD`)
- Milestones (owned by `OUTR.LAUNCH`)

## Requirements

- **REQ-OUTR.COMM.REPORT-001** The project shall publish the adoption report for a calendar quarter within 30 days after the quarter ends.
- **REQ-OUTR.COMM.REPORT-002** Each report shall show every metric in the definition file for the quarter and the three previous quarters, and shall mark a metric as unavailable, with the reason, instead of omitting it.
- **REQ-OUTR.COMM.REPORT-003** Each number in a report shall be regenerable from the stored snapshots and aggregates, and a regeneration check shall fail if a published number differs from a fresh regeneration.
- **REQ-OUTR.COMM.REPORT-004** Each report shall link the metric definition of every metric it shows, including its known biases.
- **REQ-OUTR.COMM.REPORT-005** Every report shall pass the OUTR.BRAND claim lint with zero errors.

## Interfaces

- **Offers:** quarterly report pages (hosted by OUTR.SITE).
- **Consumes:** OUTR.COMM.METRIC snapshots; OUTR.COMM.SURVEY aggregates; OUTR.COMM.MOD quarterly counts; OUTR.LAUNCH milestone dates; OUTR.BRAND.LINT.

## Decomposition hints

GEN (report generator), REGEN (regeneration check), PUB (publication and deadline reminder).

## Open questions

- Before any survey runs, the survey section is simply marked unavailable.

## Children

_None yet: status `open`._
