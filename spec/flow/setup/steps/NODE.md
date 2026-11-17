---
id: FLOW.SETUP.STEPS
title: Wizard step bindings to owning domain commands
level: 3
parent: FLOW.SETUP
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-FLOW.SETUP-002, REQ-FLOW.SETUP-003, REQ-FLOW.SETUP-006]
covers: [JOB.USE.07]
depends_on: [CORE.STO.SET, CORE.JRN.ENTRY, TRUST.PROFILE.SELECT, TRUST.SIGN.KEYSTORE, TRUST.SIGN.UV, UNC.DEC.BIND, UNC.INST.LIB, RPT.MODEL.TMPL]
uses_artifacts: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# FLOW.SETUP.STEPS: Wizard step bindings to owning domain commands

## Purpose

The table that binds each of wizard steps 1 to 7 to the registry commands of the owning domain, the defaults shown, and whether keeping the default is allowed. The wizard never changes a setting itself: a step counts as done only when the journal shows one of its bound commands committed since the wizard started, or when the user keeps an allowed default. This is what keeps the wizard from being a side door (CONSTITUTION C3).

## Scope

**In:**
- `StepBinding` table for steps 1 to 7: bound commands, setting keys, defaults, `kept_default` allowed or not
- Default resolution: display units and date format from the OS locale; profile `lean`; agent access off; export-control tagged data default-deny; no ASME rule
- `is_step_satisfied(step, journal_since_start, mode)` predicate
- The principal rule: steps 2 to 7 need a human principal

**Out:**
- Wizard state and commands (owned by `FLOW.SETUP.WIZARD`)
- Step 4 skip acknowledgement (owned by `FLOW.SETUP.RULEGATE`)
- The bound commands themselves (owned by `CORE.STO.SET`, `TRUST.PROFILE.SELECT`, `TRUST.SIGN.KEYSTORE`, `UNC.DEC.BIND`, `UNC.INST.LIB`, `RPT.MODEL.TMPL`)

## Requirements

- **REQ-FLOW.SETUP.STEPS-001** The binding table shall bind: step 1 to `settings.set_value@1` for keys `org.name`, `site.name`, `display.units`, `display.date_format`; step 2 to `sign.set_profile@1`; step 3 to `sign.create_key@1`; step 4 to `decide.set_default_rule@1`; step 5 to the instrument-profile library commands of `UNC.INST.LIB`; step 6 to `settings.set_value@1` for key `report.default_template` with templates listed by `report.list_template@1`; step 7 to `settings.set_admin_value@1` for keys `agent.access` and `agent.export_controlled`.
- **REQ-FLOW.SETUP.STEPS-002** `is_step_satisfied` shall return true in mode `changed` only if a bound command for that step was committed in the journal after the wizard-start entry by the same human principal.
- **REQ-FLOW.SETUP.STEPS-003** Mode `kept_default` shall be allowed for steps 1, 2, 5, 6 and 7, allowed for step 3 only while the active profile does not require signatures, and never allowed for step 4.
- **REQ-FLOW.SETUP.STEPS-004** The defaults shown shall be: display units and date format from the OS locale, profile `lean`, agent access `off`, export-control tagged data `deny`, and no ASME default decision rule.
- **REQ-FLOW.SETUP.STEPS-005** The binding table shall mark steps 2 to 7 as requiring a human principal, so that the wizard refuses them for agent and script principals.

## Acceptance criteria

- **AC-1** Given fixture journal `fixtures/flow/setup/step2-done.jrn` (wizard start, then `sign.set_profile@1` by a human), when `is_step_satisfied(2, changed)` runs, then true.
- **AC-2** (error path) Given the same journal with `sign.set_profile@1` before wizard start, then false.
- **AC-3** (error path) Given `kept_default` for step 4, then the predicate returns `not_allowed`.
- **AC-4** Given an OS locale `de-DE` fixture, then the step 1 defaults are `mm` and `dd.MM.yyyy` [unverified mapping table].
- **AC-5** Given step 3 `kept_default` with the `regulated` profile active, then `not_allowed`.

## Design

- **Registry command(s):** none (library used by the wizard commands).
- **Data structures:** `StepBinding { step: u8, commands: Vec<CommandRef>, keys: Vec<String>, default: DefaultSpec, kept_default: Allowed|Never|IfProfileAllows, human_only: bool }`.
- **Algorithm:** scan journal entries after the wizard-start entry for bound command IDs and setting keys, check principal.
- **Errors:** `STEP-UNKNOWN`, `STEP-NOT-ALLOWED`.
- **Determinism:** D0 (the OS locale is an input captured once at wizard start).
- **Performance budget:** negligible.
- **Crate:** `fid-setup` (L3, FLOW).

## Test fixtures

Synthetic journals under `fixtures/flow/setup/`; a locale-to-default table fixture.

## Build prompt

> Create crate `fid-setup` (layer L3) with module `steps`: a static binding table for wizard steps 1..7 (step 1 settings.set_value@1 keys org.name, site.name, display.units, display.date_format; 2 sign.set_profile@1; 3 sign.create_key@1; 4 decide.set_default_rule@1; 5 instrument-profile library commands; 6 settings.set_value@1 key report.default_template; 7 settings.set_admin_value@1 keys agent.access, agent.export_controlled), defaults (locale units/date, lean, agent access off, export-controlled deny, no ASME rule), kept_default policy (allowed 1, 2, 5, 6, 7; step 3 only if the profile does not require signatures; never 4), human-only for steps 2..7, and `is_step_satisfied(step, journal_since_start, mode, principal)`. Tests AC-1..AC-5 of spec node FLOW.SETUP.STEPS. No panic, no `unwrap`.

## Open questions

- Resolution of the parent's side-door question: `setup.complete_step@1` carries no setting value; it is accepted only when this predicate holds, so every setting is still made by its owning command.
- Signing identity on Linux follows D-011 (accepted: hardware token where present, otherwise passphrase, method recorded; Regulated requires the token). Step 3 is therefore always completable on Linux under Lean.
- `UNC.INST.LIB` names only `unc.list_profile@1` so far; step 5's bound write commands are taken from that node when it is expanded.

## Children

_Leaf: none._
