---
id: UX.PATTERN.GUIDE
title: Onboarding and in-app guidance pattern
level: 3
parent: UX.PATTERN
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-UX.PATTERN-001, REQ-UX.PATTERN-010]
covers: []
depends_on: [UX.PATTERN.SCHEMA, UX.COMP.CONT]
uses_artifacts: [FLOW.SETUP.JOURNEY, FLOW.SETUP.ONBOARD]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.PATTERN.GUIDE: Onboarding and in-app guidance pattern

## Purpose

How the apps guide a newcomer without getting in the way: the first-run flow around Shop Setup, empty states that offer the next command, contextual help, and step-mode tours handed off from FLOW's first tutorial. Guidance is always dismissible, recoverable from help, and never covers a control the current step needs.

## Scope

**In:** `patterns/guide.md` (`pat.guide`), `patterns/empty-state.md` (`pat.empty`).
**Out:** Shop Setup steps and tutorial content (FLOW.SETUP); tutorials outside the app (OUTR.CONTENT); Shop Setup screens (`UX.SCREENS.COMMON`).

## Requirements

- **REQ-UX.PATTERN.GUIDE-001** Every guidance element shall have a dismiss control reachable by keyboard, and every dismissed guide shall be listed and restartable from the help menu.
- **REQ-UX.PATTERN.GUIDE-002** A tour step shall never overlap the control that the step asks the user to use; when space is short, the tour callout shall move or dock.
- **REQ-UX.PATTERN.GUIDE-003** Each empty state shall say what the view will show and offer at least one command that fills it.
- **REQ-UX.PATTERN.GUIDE-004** The first-run flow shall follow the Shop Setup journey's step order and resume where the user left off.
- **REQ-UX.PATTERN.GUIDE-005** Both specs shall pass `fid-design pattern-check`.

## Acceptance criteria

- **AC-1** Given the two specs, when `fid-design pattern-check --journeys` runs with the Shop Setup journey map, then it exits 0 with no `PAT-JOURNEY-UNKNOWN`.
- **AC-2** Given `pat.guide`, then the dismiss, recover and no-overlap rules are present (review items).

## Design

- **Deliverable:** two Markdown specs in `pattern-spec@1`, citing the Shop Setup and onboarding journey steps.
- **Determinism:** D0.
- **Crate:** none (package content).

## Test fixtures

The shipped specs and a copy of the setup journey map ID list; CC-BY-4.0.

## Build prompt

> Write `patterns/guide.md` (first run following the Shop Setup journey order with resume; contextual help; step-mode tours handed off from the first tutorial; every guide dismissible by keyboard, listed and restartable in help; callouts never overlap the control the step needs) and `patterns/empty-state.md` (what the view will show plus at least one filling command) in `pattern-spec@1`. Run `fid-design pattern-check --journeys <dir>` until it exits 0, then open a design-review record.

## Open questions

- None.

## Children

_Leaf: none._
