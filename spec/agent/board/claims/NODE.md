---
id: AGENT.BOARD.CLAIMS
title: Claim register and claim check
level: 3
parent: AGENT.BOARD
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-eval-board]
built_pr: 110
satisfies: [REQ-AGENT.BOARD-005]
covers: []
depends_on: []
uses_artifacts: [AGENT.BOARD.BUNDLE]
side_effect: R0
determinism: D0
cli: [fid-eval board check-claims]
mcp: []
journal: []
updated: 2026-11-17
---

# AGENT.BOARD.CLAIMS: Claim register and claim check

## Purpose

Keeps public statements about agent capability honest. Every such statement in docs, website and release notes carries a claim tag, and the register maps each tag to a scoreboard entry whose figure supports it. The check fails when a tagged claim has no entry, the entry no longer supports it, or untagged capability wording appears.

## Scope

**In:**
- `agent/claims.json`: claim ID, wording summary, board release, model, scope (task, family or app), metric, threshold
- Claim tag syntax in Markdown and HTML sources: `<!-- fid-claim: <id> -->`
- `fid-eval board check-claims --register <file> --board <bundle> --sources <dir>...`

**Out** (and which node owns it instead):
- Writing docs and website text (`OUTR`)
- Running the check in CI for those sources (ECO.CI wiring, OUTR sources)

## Requirements

- **REQ-AGENT.BOARD.CLAIMS-001** Each register entry shall name a board release, a model, a scope, a metric (success rate, adoptability rate, human-adoption rate or violation count) and a threshold the published figure must meet.
- **REQ-AGENT.BOARD.CLAIMS-002** If a claim tag in the sources names an ID absent from the register, or a register entry's figure in the named board does not meet its threshold, then the check shall fail naming the claim and source location.
- **REQ-AGENT.BOARD.CLAIMS-003** If a source sentence contains an agent-capability phrase from the project's phrase list and no claim tag within the same paragraph, then the check shall fail with `untagged_claim` [unverified heuristic].
- **REQ-AGENT.BOARD.CLAIMS-004** If a register entry names a model whose release verdict is `block` or `incomplete`, then the check shall fail with `claim_on_blocked_release`.

## Acceptance criteria

- **AC-1** Given `fixtures/agent/board/claims/ok/` (register, bundle, docs with tags), when the check runs, then it passes.
- **AC-2** (error path) Given a doc tag `fid-claim: c9` absent from the register, then it fails naming the file and line.
- **AC-3** (error path) Given a register threshold of 0.9 against a published 0.85 rate, then it fails naming the claim.
- **AC-4** (error path) Given a paragraph "our agent programs a part from PMI" without a tag, then it fails with `untagged_claim`.

## Design

- **Registry command(s):** none (D-032).
- **Data structures:** `Claim { id, summary, release, model, scope, metric, threshold: Fraction }`; phrase list `agent/claim-phrases.txt` (project-authored).
- **Algorithm:** parse register; load board; compare exact fractions; scan sources for tags and phrases paragraph by paragraph.
- **Errors:** `unknown_claim`, `threshold_not_met`, `untagged_claim`, `claim_on_blocked_release`.
- **Determinism:** D0.
- **Performance budget:** < 10 s over the docs and website sources [unverified].
- **Crate:** `fid-eval-board`

## Test fixtures

Project-authored register, bundle and Markdown sources under `fixtures/agent/board/claims/`.

## Build prompt

> In crate `fid-eval-board` (Tooling), implement `fid-eval board check-claims --register agent/claims.json --board <bundle> --sources <dir>...`. Register entries name release, model, scope, metric (success, adoptability, human-adoption, violations) and a threshold compared as exact fractions with the bundle's figure. Fail on unknown claim tags (`<!-- fid-claim: id -->`), unmet thresholds, claims on releases whose red-team verdict is `block` or `incomplete`, and capability phrases from `agent/claim-phrases.txt` with no tag in the same paragraph (`untagged_claim`). Report file and line. Acceptance: AC-1 to AC-4 of AGENT.BOARD.CLAIMS. Constraints: CONSTITUTION §8; generated, never hand-edited status (§8 honest status).

## Open questions

- OUTR and ECO must run this check on their sources and release notes; no OUTR or ECO node states that yet.

## Children

_Leaf: none._
