---
id: FLOW.RUNNER.CHKUI
title: UI and presentation-track quality-check evaluators
level: 3
parent: FLOW.RUNNER
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-FLOW.RUNNER-002]
covers: []
depends_on: [FLOW.RUNNER.PKG, AUTO.CTRL.UIOPS, VIZ.SHELL.A11Y]
uses_artifacts: [VIZ.CAPTURE.TRACK]
side_effect: R0
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# FLOW.RUNNER.CHKUI: UI and presentation-track quality-check evaluators

## Purpose

Evaluates the quality checks that depend on what the user sees or does, not on stored values: that no editing command is reachable from a locked screen, that no pointer event occurred during keyboard-only step ranges, and that each displayed result carries the badge of its evaluation basis.

## Scope

**In:**
- `no_edit_reachable`: walk the accessibility tree of the current screen at each listed step
- `no_pointer_events`: read the presentation track for the declared step ranges
- `basis_label`: match displayed basis badges with record evaluation basis

**Out:**
- Value checks (owned by `FLOW.RUNNER.CHKVAL`)
- The accessibility tree, control channel and presentation track (owned by `VIZ.SHELL.A11Y`, `AUTO.CTRL.UIOPS`, `VIZ.CAPTURE.TRACK`)

## Requirements

- **REQ-FLOW.RUNNER.CHKUI-001** `no_edit_reachable` shall fail at a step if any action reachable in the accessibility tree names a command of class W2 or higher that the package does not list as an allowed operator command.
- **REQ-FLOW.RUNNER.CHKUI-002** `no_pointer_events` shall fail if the presentation track contains any pointer-move, pointer-button or wheel event whose journal-clock time falls within a declared step range.
- **REQ-FLOW.RUNNER.CHKUI-003** `basis_label` shall fail for any displayed result whose badge does not match its ResultRecord `evaluation_basis`.
- **REQ-FLOW.RUNNER.CHKUI-004** If the package has no presentation track or the app has no accessibility tree available, then the evaluator shall return `error`, never `pass`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/flow/chkui/kiosk-ok/` (locked screen), then `no_edit_reachable` passes; given `kiosk-leak/` (an exposed `report.add_balloon@1` action), then it fails naming the command.
- **AC-2** Given `track-kbd.ptrk` (no pointer events), then `no_pointer_events` passes; given `track-click.ptrk`, then it fails naming the step.
- **AC-3** Given a 3-result screen with one RESULTS_ONLY result shown with the POINTS badge, then `basis_label` fails.
- **AC-4** (error path) Given a package without a track, then `no_pointer_events` returns `error`.

## Design

- **Registry command(s):** none (library); uses control-channel UI inspection.
- **Data structures:** `UiCheckOutcome` as in CHKVAL.
- **Algorithm:** at listed steps, request the accessibility tree; filter actions by `command` attribute and look up class in the manifest; parse presentation-track events and map times to step ranges by journal clock.
- **Errors:** `CHKUI-NO-TRACK`, `CHKUI-NO-A11Y`, `CHKUI-PARAM`.
- **Determinism:** D1 (accessibility-tree order may differ by platform; outcomes are sorted).
- **Performance budget:** tree walk under 200 ms per step [unverified].
- **Crate:** `fid-accept` module `checks::ui`.

## Test fixtures

Fixture packages and presentation tracks under `fixtures/flow/chkui/` (synthetic).

## Build prompt

> In crate `fid-accept`, add module `checks::ui` with evaluators `no_edit_reachable` (accessibility tree via the control channel; fail on reachable actions whose command is class W2+ and not in the package's allowed operator commands), `no_pointer_events` (presentation-track pointer and wheel events within declared step ranges, matched by journal clock) and `basis_label` (badge vs `evaluation_basis`). Missing track or tree gives `error`. Tests AC-1..AC-4 of spec node FLOW.RUNNER.CHKUI. No panic, no `unwrap`.

## Open questions

- Needs the accessibility tree to expose a `command` attribute on actions and the basis badge as an attribute; requests to `VIZ.SHELL.A11Y` (shared with `FLOW.DEADEND.WHY`).

## Children

_Leaf: none._
