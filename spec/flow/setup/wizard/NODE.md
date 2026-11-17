---
id: FLOW.SETUP.WIZARD
title: Wizard state machine, commands and resume
level: 3
parent: FLOW.SETUP
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-FLOW.SETUP-001, REQ-FLOW.SETUP-003, REQ-FLOW.SETUP-006, REQ-FLOW.SETUP-007]
covers: [JOB.USE.07]
depends_on: [FLOW.SETUP.STEPS, CORE.STO.SET, CORE.BUS.PIPE, CORE.JRN.ENTRY]
uses_artifacts: []
side_effect: [W2, R0, W2, W2]
determinism: D0
cli: [fid setup start-wizard, fid setup show-status, fid setup complete-step, fid setup defer-wizard]
mcp: [setup_start_wizard, setup_show_status, setup_complete_step, setup_defer_wizard]
journal: [setup.start_wizard@1, setup.show_status@1, setup.complete_step@1, setup.defer_wizard@1]
updated: 2026-11-17
---

# FLOW.SETUP.WIZARD: Wizard state machine, commands and resume

## Purpose

The Shop Setup state machine and its registry commands: offering the wizard on first run before any project can be created, presenting steps 1 to 8 in order, recording each step as done, kept-default or acknowledged-skip, resuming after an interruption at the first unfinished step, and allowing the wizard to be deferred only by an explicit journaled choice.

## Scope

**In:**
- `setup.start_wizard@1` (W2), `setup.show_status@1` (R0), `setup.complete_step@1` (W2), `setup.defer_wizard@1` (W2)
- Wizard state in machine-scoped settings: `not_started | in_progress | deferred | completed`, per-step status
- First-run gate: project creation is offered the wizard first
- Resume after crash, kill or close
- MCP exposure: only `setup_show_status`

**Out:**
- Step bindings and defaults (owned by `FLOW.SETUP.STEPS`)
- Step 4 skip (owned by `FLOW.SETUP.RULEGATE`); step 8 (owned by `FLOW.SETUP.SAMPLE`)
- Wizard screens (owned by `UX`)

## Requirements

- **REQ-FLOW.SETUP.WIZARD-001** When a project-creation command is invoked by a human while the wizard state is `not_started`, the system shall return outcome `not_run` with reason `setup.not_started` and suggested fixes `setup.start_wizard@1` and `setup.defer_wizard@1`.
- **REQ-FLOW.SETUP.WIZARD-002** `setup.defer_wizard@1` shall be human-only, journaled, and set the state to `deferred`; while `deferred`, project creation shall proceed and `show_status` shall report the deferral.
- **REQ-FLOW.SETUP.WIZARD-003** `setup.complete_step@1` shall accept step `n` only if steps 1 to n-1 are done, kept-default or acknowledged-skip, and only if `is_step_satisfied` holds for the requested mode; otherwise it shall fail with `setup.step_order` or `setup.step_unsatisfied` and journal nothing.
- **REQ-FLOW.SETUP.WIZARD-004** `setup.complete_step@1` shall take only the step number and mode, carry no setting value, and record the journal hash of the satisfying bound command when the mode is `changed`.
- **REQ-FLOW.SETUP.WIZARD-005** When the app starts with state `in_progress`, the system shall resume at the first step not done, kept-default or acknowledged-skip, with the settings of earlier steps unchanged.
- **REQ-FLOW.SETUP.WIZARD-006** `setup.start_wizard@1`, `setup.complete_step@1` and `setup.defer_wizard@1` shall be declared `human_only`, so that the bus refuses agent and script principals and MCP offers them only as request-only tools, leaving `setup_show_status` the only directly callable wizard tool.
- **REQ-FLOW.SETUP.WIZARD-007** When every step 1 to 8 is done, kept-default or acknowledged-skip, the system shall set the state to `completed`.

## Acceptance criteria

- **AC-1** Given a fresh settings store, when `fid project create-from-template` runs as a human, then the outcome is `not_run` with `setup.not_started` and both fixes.
- **AC-2** Given steps 1 and 2 complete, when the process is killed and restarted, then `fid setup show-status --json` reports step 3 as current and steps 1 and 2 done with their recorded hashes.
- **AC-3** (error path) Given step 2 not done, when `fid setup complete-step 3 --mode changed` runs, then it fails with `setup.step_order` and nothing is journaled.
- **AC-4** (error path) Given an MCP agent session, when tools are listed, then `setup_show_status` is the only directly callable wizard tool and the others appear only as request-only tools; invoking `setup.complete_step@1` as the agent is refused and nothing is journaled.
- **AC-5** (replay) Given the journal of AC-2, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `setup.start_wizard@1` (W2, human_only, no params); `setup.show_status@1` (R0); `setup.complete_step@1` (W2, human_only; `step: 1..8`, `mode: changed|kept_default`); `setup.defer_wizard@1` (W2, human_only). Step 8 completion is recorded by `setup.run_sample_job@1` or `kept_default` (skip sample).
- **Data structures:** `WizardState { status, started_entry, steps: [StepStatus; 8] }` with `StepStatus = pending|done{hash}|kept_default|acknowledged_skip{hash}`; stored as a machine-scope setting.
- **Algorithm:** a total state machine; precondition on project-creation commands via the bus precondition hook.
- **Errors:** `setup.not_started`, `setup.step_order`, `setup.step_unsatisfied`, `setup.already_completed`.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-setup`.

## Test fixtures

Fresh and partially configured settings stores under `fixtures/flow/setup/`.

## Build prompt

> In crate `fid-setup`, add module `wizard` and commands `setup.start_wizard@1` (W2, human_only), `setup.show_status@1` (R0; MCP `setup_show_status`; the three human-only commands appear on MCP only as generated request-only tools), `setup.complete_step@1` (W2, human_only; params step 1..8, mode changed|kept_default; no setting values), `setup.defer_wizard@1` (W2, human_only); CLI `fid setup start-wizard|show-status|complete-step|defer-wizard`. State in machine-scope settings: not_started|in_progress|deferred|completed with per-step pending|done{hash}|kept_default|acknowledged_skip{hash}. Use the steps module's `is_step_satisfied`; enforce step order; resume at first unfinished step after restart; register a precondition on project creation returning `not_run` with reason `setup.not_started` and fixes start/defer. Tests AC-1..AC-5 of spec node FLOW.SETUP.WIZARD. CONSTITUTION §3 human-only rules; no panic, no `unwrap`.

## Open questions

- Whether deferring setup should be allowed at all under the Regulated profile is a governance question for `TRUST.PROFILE`; this leaf allows it for any profile until told otherwise.

## Children

_Leaf: none._
