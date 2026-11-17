---
id: ECO.CI.BOTS
title: Automated review bots and finding triage
level: 3
parent: ECO.CI
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO.CI-005]
covers: []
depends_on: [ECO.CI.WF]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.CI.BOTS: Automated review bots and finding triage

## Purpose

Automated reviewers (code review and static analysis) that comment on every PR, configured so that their findings are advisory by default, a maintainer can mark one blocking, and no standards text or excluded material is ever sent to an external service. It owns the bot selection criteria, their configuration and the blocking-finding mechanism.

## Scope

**In:**
- Selection criteria for review and static-analysis bots (licence terms, data handling, offline or self-hosted options)
- Bot configuration, including path exclusions for any material that must not leave the project's control
- The mechanism by which a maintainer marks a finding blocking, and the check that enforces it
- The rule that bot comments do not start the PR-timeline clock (consumed by ECO.COMM)

**Out** (and which node owns it instead):
- PR-timeline bot (owned by `ECO.COMM`)
- Lint and licence gates (owned by `ECO.CI.LINT`, `ECO.CI.LIC`)
- Workflow wiring (owned by `ECO.CI.WF`)

## Requirements

- **REQ-ECO.CI.BOTS-001** When a PR is opened or updated, at least one automated reviewer shall post its findings as review comments within the CI time budget.
- **REQ-ECO.CI.BOTS-002** A bot finding shall not block merging unless a maintainer marks it blocking, and while a blocking-marked finding is unresolved the required check `bot-blocking` shall fail.
- **REQ-ECO.CI.BOTS-003** Bot configuration shall exclude every path listed as restricted (standards excerpts, customer data, unpublished datasets) from content sent to any external service, and a test PR touching a restricted path shall show no excerpt of it in the bot's request log [unverified that each candidate bot exposes such a log].
- **REQ-ECO.CI.BOTS-004** Every bot used shall be recorded with its licence or terms, data-handling statement and the date it was reviewed.

## Interfaces

- **Offers:** bot configuration; `bot-blocking` required check; bot register.
- **Consumes:** ECO.CI.WF workflows; restricted-path list (house rules, ECO.RULES).

## Decomposition hints

SELECT (criteria and register), CONF (configuration and exclusions), BLOCK (blocking-finding mechanism).

## Open questions

- Which hosted bots are acceptable under C7 and the standards-text rule; a self-hosted static analyser may be preferable [unverified that hosted bots honour path exclusions].
- Whether choosing hosted third-party services for PR review needs a maintainer decision record; likely yes once a bot needs repository write access.

## Children

_None yet: status `open`._
