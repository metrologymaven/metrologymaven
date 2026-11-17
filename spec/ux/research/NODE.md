---
id: UX.RESEARCH
title: User research and usability testing
level: 2
parent: UX
kind: subsystem
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-UX-005]
covers: []
depends_on: [FLOW]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.RESEARCH: User research and usability testing

## Purpose

The evidence that the design works for real metrologists and operators: interviews and contextual inquiry before the first workflow rail is frozen, a usability test of every release on the acceptance jobs, SUS tracking across releases, and published findings. It is separate because it is a recurring research programme with its own protocol, consent and data-handling rules, and because it starts in P0, before any screen is built.

## Scope

**In:**
- Research plan, recruitment criteria and participant consent
- Interviews and contextual inquiry with practising metrologists and operators
- Per-release usability tests on FLOW's acceptance jobs: task success, time, errors, SUS
- Severity rating scale and the issue log, linked to spec nodes
- SUS tracking and published release research reports
- Participant data handling and anonymisation

**Out** (and which node owns it instead):
- Acceptance jobs, task-time targets and usability bars (`FLOW`)
- Design-partner timing studies (`FLOW`, REQ-FLOW-007)
- Product telemetry and opt-in surveys (`OUTR.COMM.SURVEY`, `OUTR.COMM.METRIC`)
- Fixing the issues found (the owning nodes)

## Requirements

- **REQ-UX.RESEARCH-001** Before the P1a workflow rail design is approved, the project shall complete at least 5 interviews or contextual inquiries with practising metrologists or operators who use current commercial tools, and publish anonymised findings.
- **REQ-UX.RESEARCH-002** Each release shall be usability-tested with at least 5 practising metrologists or operators on the acceptance jobs of the apps it changes.
- **REQ-UX.RESEARCH-003** Each usability test shall record per task: completion, time on task, errors and assists, and per participant: a SUS questionnaire.
- **REQ-UX.RESEARCH-004** Within 30 days of each release, the project shall publish the SUS mean, participant count and 95 % confidence interval, and the top issues ranked on a defined severity scale.
- **REQ-UX.RESEARCH-005** Every top issue shall be filed as a tracked issue that names the spec node it affects.
- **REQ-UX.RESEARCH-006** If a release's SUS mean falls more than 5 points below the previous release's [unverified threshold], then the research report shall flag a usability regression.
- **REQ-UX.RESEARCH-007** The research protocol (tasks, script, metrics, recruitment criteria) shall be published so that others can repeat it.
- **REQ-UX.RESEARCH-008** Every participant shall give informed consent before a session, published findings shall contain no personal data, and recordings shall not be published without separate explicit consent.
- **REQ-UX.RESEARCH-009** Research data shall never be collected through product telemetry unless the participant has opted in (CONSTITUTION C7).
- **REQ-UX.RESEARCH-010** Research reports shall describe incumbent workflows in the project's own words and shall not publish incumbent screenshots, text or icons (CONSTITUTION C9).

## Interfaces

- **Offers:** published research findings, release usability reports, SUS history, an issue log linked to nodes; findings feed `UX.SCREENS` and FLOW personas.
- **Consumes:** `FLOW` (personas, acceptance jobs, usability bars). Interface notes (no `depends_on`): tests use released builds from `ECO.REL` and the mockups from `UX.SCREENS`; GA gating on research results is a FLOW and ECO decision, not set here.

## Decomposition hints

PLAN (research plan, recruitment, consent and data handling), INQUIRY (interviews and contextual inquiry), USAB (per-release usability tests and severity scale), SUS (SUS tracking and release reports).

## Split rationale

The dividing principle is **the research cycle**: PROTO (protocol, consent, data handling and the publication check, governing all activities), INQUIRY (the P0 discovery round before the workflow rail is approved), USAB (per-release usability sessions, severity scale and issue filing), SUS (scoring, history, regression flag and the release report). This follows the decomposition hints. PROTO and INQUIRY start in P0; USAB and SUS start in P1a, the first release with a GUI.

Alternatives considered:
- **USAB and SUS as one child.** Rejected: session recording is a field activity with a record format, while SUS reporting is a deterministic calculation with its own statistics fixtures.
- **Reusing FLOW.STUDY's record format.** Rejected for now: it would add a code edge from UX to FLOW's study crate; the formats can be merged later by decision.

## Open questions

- REQ-UX-005 does not say whether failing usability results block GA. FLOW owns GA gates (REQ-FLOW-003); a link from SUS to GA would need a maintainer decision (see UX.RESEARCH.SUS). Not opened as a decision now because nothing here is blocked by it.
- Recruiting 5 practising participants per release depends on design partners; feasibility for a volunteer project is [unverified].
- FLOW.STUDY.PROTO also measures SUS (bar 70 [unverified]) for design-partner sessions; the two programmes should share participants' consent wording and avoid double-booking partners.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [UX.RESEARCH.PROTO](proto/NODE.md) | Research plan, protocol, consent and data handling (leaf) | 007 008 009 010 | none |
| [UX.RESEARCH.INQUIRY](inquiry/NODE.md) | Interviews and contextual inquiry before the P1a rail (leaf) | 001 | none |
| [UX.RESEARCH.USAB](usab/NODE.md) | Per-release usability tests, severity scale and issue filing (leaf) | 002 003 005 | none |
| [UX.RESEARCH.SUS](sus/NODE.md) | SUS scoring, history and release research reports (leaf) | 004 006 | none |
