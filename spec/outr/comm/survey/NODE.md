---
id: OUTR.COMM.SURVEY
title: Opt-in surveys
level: 3
parent: OUTR.COMM
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.COMM-007]
covers: []
depends_on: []
uses_artifacts: [OUTR.COMM.CHAN, OUTR.BRAND.LINT]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.COMM.SURVEY: Opt-in surveys

## Purpose

Voluntary surveys of users and contributors, the only adoption input that is not a public signal. It is separate because it handles respondent data and must meet stricter rules: no personal data beyond what is entered, a stated purpose and retention, and publication of aggregates only above a minimum cell size.

## Scope

**In:**
- Survey question sets and their review
- The survey tool choice criteria (self-hostable, no tracking)
- Privacy statement per survey: purpose, retention, who sees raw responses
- Aggregation with a minimum cell size and publication of aggregates

**Out** (and which node owns it instead):
- Public-signal metrics (owned by `OUTR.COMM.METRIC`)
- The quarterly report layout (owned by `OUTR.COMM.REPORT`)
- Any in-product prompt or telemetry (not allowed here; CONSTITUTION C7)

## Requirements

- **REQ-OUTR.COMM.SURVEY-001** Each survey shall state, before the first question, its purpose, its retention period for raw responses and who can read them.
- **REQ-OUTR.COMM.SURVEY-002** A survey shall collect no personal data beyond what the respondent types into its fields, and shall not record IP addresses, device fingerprints or third-party cookies.
- **REQ-OUTR.COMM.SURVEY-003** If a reported cell of a survey aggregate has fewer than 10 respondents [unverified threshold], then the cell shall be suppressed or merged before publication.
- **REQ-OUTR.COMM.SURVEY-004** Raw survey responses shall be deleted at the end of the stated retention period, and the deletion date shall be recorded.
- **REQ-OUTR.COMM.SURVEY-005** Surveys shall be announced only on official channels and on the website, never from inside the product.

## Interfaces

- **Offers:** survey aggregates for OUTR.COMM.REPORT.
- **Consumes:** OUTR.COMM.CHAN channels for announcements; OUTR.BRAND.LINT for survey texts.

## Decomposition hints

TOOL (tool criteria and setup), QSET (question sets), AGG (aggregation and suppression).

## Open questions

- The 10-respondent threshold is a proposal (carried from OUTR.COMM).
- Phase P1a: a survey is useful only once there are users of a usable app.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [OUTR.COMM.SURVEY.TOOL](tool/NODE.md) | Survey tool criteria and tracking check (leaf) | 002 | none |
| [OUTR.COMM.SURVEY.QSET](qset/NODE.md) | Survey definitions, privacy statement and announcements (leaf) | 001 002 005 | none |
| [OUTR.COMM.SURVEY.AGG](agg/NODE.md) | Survey aggregation, cell suppression and deletion record (leaf) | 003 004 | none |
