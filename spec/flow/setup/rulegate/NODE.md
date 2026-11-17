---
id: FLOW.SETUP.RULEGATE
title: Decision-rule skip acknowledgement and rule_missing link
level: 3
parent: FLOW.SETUP
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-FLOW.SETUP-004, REQ-FLOW.SETUP-005]
covers: [JOB.USE.07]
depends_on: [FLOW.SETUP.WIZARD, UNC.DEC.DEFAULT]
uses_artifacts: [FLOW.DEADEND.CATALOG]
side_effect: W2
determinism: D0
cli: [fid setup acknowledge-skip]
mcp: [setup_acknowledge_skip]
journal: [setup.acknowledge_skip@1]
updated: 2026-11-17
---

# FLOW.SETUP.RULEGATE: Decision-rule skip acknowledgement and rule_missing link

## Purpose

Makes it impossible to leave wizard step 4 without an ASME default decision rule by accident. Leaving it without a rule needs a journaled, human-only acknowledgement that states the consequence: ASME results that need a rule will be NOT_DECIDABLE with reason `rule_missing`. It also checks that the suggested fix for `rule_missing` leads back to setting a default rule.

## Scope

**In:**
- `setup.acknowledge_skip@1` (W2, human_only) for step 4
- The consequence statement recorded with the acknowledgement (dialects left without a rule)
- A test that the `decide.rule_missing` catalogue entry names `decide.set_default_rule@1`

**Out:**
- The catalogue entry itself (owned by `FLOW.DEADEND.CATALOG`)
- Rule resolution and `rule_missing` (owned by `UNC.DEC.DEFAULT`)

## Requirements

- **REQ-FLOW.SETUP.RULEGATE-001** When `setup.acknowledge_skip@1` is invoked for step 4 while no ASME default decision rule is set, the system shall record step 4 as acknowledged-skip with the list of dialects left without a default rule and the consequence code `decide.rule_missing`.
- **REQ-FLOW.SETUP.RULEGATE-002** If an ASME default decision rule is already set, then `setup.acknowledge_skip@1` shall fail with `setup.nothing_to_skip` and journal nothing.
- **REQ-FLOW.SETUP.RULEGATE-003** `setup.acknowledge_skip@1` shall be declared `human_only` and shall not be projected as an MCP tool other than request-only.
- **REQ-FLOW.SETUP.RULEGATE-004** The suggested-fix catalogue entry for `decide.rule_missing` shall include `decide.set_default_rule@1`, checked by a test that reads the shipped catalogue.
- **REQ-FLOW.SETUP.RULEGATE-005** Only steps that a binding marks as never keepable by default (step 4) shall accept `setup.acknowledge_skip@1`; any other step shall fail with `setup.skip_not_applicable`.

## Acceptance criteria

- **AC-1** Given steps 1 to 3 done and no ASME rule, when `fid setup acknowledge-skip 4` runs as a human, then step 4 is acknowledged-skip and the journal entry records `decide.rule_missing` and dialect `asme`.
- **AC-2** Given the state of AC-1, when an ASME characteristic is evaluated, then its result is NOT_DECIDABLE with `rule_missing` and the result's suggested fixes include `decide.set_default_rule@1`.
- **AC-3** (error path) Given an ASME default rule set, then the command fails with `setup.nothing_to_skip`.
- **AC-4** (error path) Given an agent principal, then the command is refused and nothing is journaled.

## Design

- **Registry command(s):** `setup.acknowledge_skip@1`, W2, human_only; params `step: u8` (only 4 accepted). Result: the recorded consequence.
- **Data structures:** `SkipAck { step, dialects_without_rule: Vec<Dialect>, consequence: "decide.rule_missing" }`.
- **Algorithm:** query `decide.list_rule@1` defaults per dialect; build the consequence; update wizard state.
- **Errors:** `setup.nothing_to_skip`, `setup.skip_not_applicable`, `setup.step_order`.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-setup`.

## Test fixtures

Settings stores with and without an ASME default rule; the shipped fix catalogue.

## Build prompt

> In crate `fid-setup`, add `setup.acknowledge_skip@1` (W2, human_only; CLI `fid setup acknowledge-skip <step>`; MCP `setup_acknowledge_skip` as a request-only tool, generated from `human_only`). Accept only step 4 (`setup.skip_not_applicable` otherwise); fail `setup.nothing_to_skip` if an ASME default rule exists (query decide.list_rule@1); otherwise record acknowledged-skip with dialects lacking a default and consequence `decide.rule_missing`. Add a test that the shipped fix catalogue maps `decide.rule_missing` to include `decide.set_default_rule@1`. Tests AC-1..AC-4 of spec node FLOW.SETUP.RULEGATE. No panic, no `unwrap`.

## Open questions

- REQ-FLOW.SETUP-005 allows the fix to "open wizard step 4"; opening a screen is a UI action, not a command, so this leaf uses the equivalent command `decide.set_default_rule@1`.

## Children

_Leaf: none._
