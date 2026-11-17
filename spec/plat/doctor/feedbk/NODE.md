---
id: PLAT.DOCTOR.FEEDBK
title: Opt-in crash and issue reporting
level: 3
parent: PLAT.DOCTOR
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT.DOCTOR-009]
covers: []
depends_on: [PLAT.DOCTOR.REPORT]
cli: [fid doctor send-report]
mcp: [doctor_prepare_report]
journal: [doctor.send_report]
updated: 2026-11-16
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

## Requirements

- **REQ-PLAT.DOCTOR.FEEDBK-001** When the user opts in, the system shall send crash reports and user-initiated issue reports (optionally with a redacted journal excerpt and the doctor report) to the project's issue intake, and shall send nothing without explicit per-report consent.

## Interfaces

To be refined when this node is expanded.

## Decomposition hints

Split along the Scope items above.

## Open questions

- Added in the level-3 review (2026-11-16) from the adoption gap analysis; not yet expanded.

## Children

_None yet: status `open`._
