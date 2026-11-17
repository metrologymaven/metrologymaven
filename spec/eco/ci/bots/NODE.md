---
id: ECO.CI.BOTS
title: Automated review bots and finding triage
level: 3
parent: ECO.CI
kind: module
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO.CI-005]
covers: []
depends_on: [ECO.CI.WF]
uses_artifacts: [ECO.RULES.SRC]
cli: []
mcp: []
journal: []
updated: 2026-11-17
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
- PR-timeline bot (owned by `ECO.COMM.TIMELINE`)
- Lint and licence gates (owned by `ECO.CI.LINT`, `ECO.CI.LIC`)
- Workflow wiring (owned by `ECO.CI.WF`)
- The restricted-path list itself (owned by `ECO.RULES.SRC`)

## Requirements

- **REQ-ECO.CI.BOTS-001** When a PR is opened or updated, at least one automated reviewer shall post its findings as review comments within the CI time budget.
- **REQ-ECO.CI.BOTS-002** A bot finding shall not block merging unless a maintainer marks it blocking, and while a blocking-marked finding is unresolved the required check `bot-blocking` shall fail.
- **REQ-ECO.CI.BOTS-003** Bot configuration shall exclude every path listed as restricted (standards excerpts, customer data, unpublished datasets) from content sent to any external service, and a test PR touching a restricted path shall show no excerpt of it in the bot's request log [unverified that each candidate bot exposes such a log].
- **REQ-ECO.CI.BOTS-004** Every bot used shall be recorded with its licence or terms, data-handling statement and the date it was reviewed.

## Interfaces

- **Offers:** `bots.toml` register (bot IDs, forge accounts, terms, data handling, review dates; the account list is read by ECO.COMM.TIMELINE); `cargo xtask bots-check`; the self-hosted reviewer's `review-findings.json` v1 and its poster workflow; the `bot-blocking` required check.
- **Consumes:** ECO.CI.WF workflows and fork-safety rules; `house-rules/restricted-paths.txt` from ECO.RULES.SRC (artifact).

## Decomposition hints

SELECT (criteria and register), CONF (configuration and exclusions), BLOCK (blocking-finding mechanism).

## Split rationale

Three tools with different inputs: REG checks the register and every bot's exclusions (static files), REVIEW is the self-hosted reviewer that satisfies REQ-001 without any external service, BLOCK evaluates exported review threads. SELECT and CONF were merged into REG because the exclusion check needs the register to know which bots send content out.

## Open questions

- Hosted third-party bots are optional: REQ-001 is met by the self-hosted reviewer (ECO.CI.BOTS.REVIEW), so no service has to be chosen now. Admitting a hosted bot is a register entry approved by a maintainer in a PR; if a hosted bot ever needs repository write access, a decision record is advisable [unverified that hosted bots honour path exclusions].

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| ECO.CI.BOTS.REG | Bot register and restricted-path exclusion check (leaf) | 003 004 | none |
| ECO.CI.BOTS.REVIEW | Self-hosted reviewer and comment poster (leaf) | 001 003 | none |
| ECO.CI.BOTS.BLOCK | Maintainer-marked blocking findings check (leaf) | 002 | none |
