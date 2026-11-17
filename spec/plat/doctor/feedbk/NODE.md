---
id: PLAT.DOCTOR.FEEDBK
title: Opt-in crash and issue reporting
level: 3
parent: PLAT.DOCTOR
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT.DOCTOR-009]
covers: []
depends_on: [PLAT.DOCTOR.REPORT, CORE.BUS.POL.EGRESS, CORE.STO.SET, CORE.STO.CAS]
cli: [fid doctor list-reports, fid doctor prepare-report, fid doctor send-report, fid doctor set-feedback-consent]
mcp: [doctor_list_reports, doctor_prepare_report, doctor_send_report, doctor_set_feedback_consent]
journal: [doctor.prepare_report@1, doctor.send_report@1, doctor.set_feedback_consent@1]
updated: 2026-11-17
---

# PLAT.DOCTOR.FEEDBK: Opt-in crash and issue reporting

## Purpose

Gives users a one-click way to report a crash or a wrong result with the evidence maintainers need (doctor report, redacted journal excerpt, versions) while honouring the no-telemetry rule: nothing leaves the machine without explicit consent for that report, and every outgoing report is shown before it is sent.

## Scope

**In:**
- Crash capture to a local queue
- Issue report composer with preview
- Redaction of paths, names and customer data
- Per-report consent; X1 egress command
- Intake endpoint contract (issue tracker or self-hosted)

**Out:**
- Commercial terms, pricing and contracts (business documents outside the tree)
- The egress primitive and destination configuration (`CORE.BUS.POL.EGRESS`, `CORE.STO.SET`)
- Crash-free session statistics (`FLOW.RUNNER.RELIAB`)

## Requirements

- **REQ-PLAT.DOCTOR.FEEDBK-001** When the user opts in, the system shall send crash reports and user-initiated issue reports (optionally with a redacted journal excerpt and the doctor report) to the project's issue intake, and shall send nothing without explicit per-report consent.

## Interfaces

- **Offers:** `doctor.list_reports@1` (R0); `doctor.prepare_report@1` (R1, stages a feedback bundle and returns its full preview); `doctor.send_report@1` (X1, request-only for agents, sends through `egress.send@1`); `doctor.set_feedback_consent@1` (W2, `human_only`); settings `feedback.consent` (user, default off) and `feedback.site_policy` (machine, admin flag, A0 via `settings.set_admin_value@1`, lockable by the managed policy file); the feedback bundle schema `feedback-bundle.json` v1 and the crash record schema (read as artifacts by the intake and by `FLOW.RUNNER.RELIAB`).
- **Consumes:** `PLAT.DOCTOR.REPORT` (report and redaction); `CORE.BUS.POL.EGRESS` (`egress.send@1`); `CORE.STO.SET` (settings declarations); `CORE.STO.CAS` (staging).

## Decomposition hints

See Children.

## Open questions

- Classes follow CONSTITUTION §3.1 as amended: data leaves only through `egress.send@1` (X1); the user's consent is W2 and journaled; the site-wide policy is A0.
- `FLOW.RUNNER.RELIAB` wants crash-free session rates from aggregates. Opt-in per-report data carries only local counters since the last sent report; aggregating them on the project side is outside the product and is not specified anywhere yet (cross-domain).

## Split rationale

Split along the **path of a report**: capture (crash records queued locally, no network, no consent needed), compose (bundle building, redaction and preview, a pure staging step) and send (consent, policy and the X1 egress call). Each boundary is a privacy boundary: nothing in CRASH or COMPOSE can reach the network, and SEND cannot change what was previewed because it sends a staged digest.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| PLAT.DOCTOR.FEEDBK.CRASH | Local crash capture, queue and list command | 001 | none |
| PLAT.DOCTOR.FEEDBK.COMPOSE | Feedback bundle composer, redaction and preview | 001 | none |
| PLAT.DOCTOR.FEEDBK.SEND | Consent, site policy and send through the egress primitive | 001 | none |
