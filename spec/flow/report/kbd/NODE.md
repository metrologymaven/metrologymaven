---
id: FLOW.REPORT.KBD
title: Keyboard-only variants of J2 and J3
level: 3
parent: FLOW.REPORT
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-FLOW.REPORT-007, REQ-FLOW.REPORT-010]
covers: [JOB.USE.06]
depends_on: [VIZ.SHELL.PALETTE, VIZ.SHELL.A11Y, VIZ.CAPTURE.TRACK]
uses_artifacts: [FLOW.REPORT.J2, FLOW.REPORT.J3, FLOW.RUNNER.PKG]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# FLOW.REPORT.KBD: Keyboard-only variants of J2 and J3

## Purpose

Proves keyboard-complete data entry and characteristic mapping (`JOB.USE.06`) on the two gating Report jobs: J2 and J3 are re-recorded with no pointer device, and the presentation track shows zero pointer events during data entry and mapping.

## Scope

**In:**
- Packages `j2-kbd` and `j3-kbd` (gating, unassisted, `input_mode: keyboard_only`) on the J2 and J3 datasets
- The `no_pointer_events` check scoped to the data-entry and mapping step ranges

**Out:**
- Keymap and focus handling (owned by `VIZ.SHELL.PALETTE`, `VIZ.SHELL.A11Y`)
- Presentation track format (owned by `VIZ.CAPTURE.TRACK`)
- The J2 and J3 jobs (owned by `FLOW.REPORT.J2`, `FLOW.REPORT.J3`)

## Requirements

- **REQ-FLOW.REPORT.KBD-001** The `j2-kbd` and `j3-kbd` packages shall use the J2 and J3 datasets and reach the same final records as `j2-unassisted` and `j3-unassisted`, checked by a `records_equal` check against those packages' end states.
- **REQ-FLOW.REPORT.KBD-002** Each package shall declare a `no_pointer_events` check over the step ranges for result import mapping, balloon mapping and reading entry, which fails on any pointer event in the presentation track within those ranges.
- **REQ-FLOW.REPORT.KBD-003** Both packages shall be declared gating and unassisted.

## Acceptance criteria

- **AC-1** Given the two packages, when `fid accept run-suite --job JOB.ACC.02 --json` and the same for `JOB.ACC.03` run, then `records_equal` and `no_pointer_events` pass.
- **AC-2** (error path) Given a copy of `j2-kbd` whose presentation track has one click inside the mapping range, then `no_pointer_events` fails naming the step.

## Design

- **Registry command(s):** none; content under `acceptance/jobs/j2/kbd/` and `acceptance/jobs/j3/kbd/`.
- **Data structures:** packages per `acceptance-job@1` with `input_mode: keyboard_only` and step ranges for `no_pointer_events`.
- **Algorithm:** recorded by a person with the pointer device disconnected, with a recording session capturing the presentation track.
- **Errors:** none at runtime.
- **Determinism:** D0 for records; times observed (N).
- **Performance budget:** none.
- **Crate:** none (content).

## Test fixtures

J2 and J3 datasets.

## Build prompt

> Record `acceptance/jobs/j2/kbd/` and `acceptance/jobs/j3/kbd/` as `acceptance-job@1` packages (gating, unassisted, `input_mode: keyboard_only`) on the J2 and J3 datasets, with the pointer device disconnected and a recording session active. Checks: replay_outcome, records_equal against j2-unassisted / j3-unassisted end states, no_pointer_events over the import-mapping, balloon-mapping and reading-entry step ranges. Verify AC-1..AC-2 of spec node FLOW.REPORT.KBD.

## Open questions

- None.

## Children

_Leaf: none._
