---
id: AGENT.BOARD
title: Published scoreboard and claim register
level: 2
parent: AGENT
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-AGENT-005]
covers: []
depends_on: [AGENT.HARNESS, AGENT.REDTEAM]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# AGENT.BOARD: Published scoreboard and claim register

## Purpose

Publishes evaluation results per release and per model, with everything needed to recompute them, and keeps public claims honest: no capability may be claimed without a published score behind it. It is separate from the harness because publication has its own rules (provenance of third-party runs, recomputation, safety shown next to capability) and its own consumer (the website and docs).

## Scope

**In:**
- Scoreboard bundle: per release, per model, per task family and app, with suite and harness versions
- Raw run records alongside scores, and the recompute check
- Labelling of third-party submissions
- The claim register mapping public capability claims to board entries, and its check
- Showing red-team results and the release verdict next to capability scores

**Out** (and which node owns it instead):
- Running and scoring (`AGENT.HARNESS`); the verdict (`AGENT.REDTEAM`)
- The website and marketing text that cite claims (`OUTR`)
- Open-weight model guidance pages (`AGENT.LOCAL`)

## Requirements

- **REQ-AGENT.BOARD-001** For each release and each evaluated model, the scoreboard shall publish results per task, per family and per app tag, with the suite version, harness version, adapter, declared model name, declared settings, run count and run date.
- **REQ-AGENT.BOARD-002** Each published score shall ship with its run records, and a recompute check shall reproduce every published score from them exactly or fail.
- **REQ-AGENT.BOARD-003** A score submitted by a third party shall be labelled with its submitter and shall be published only if its run records pass the recompute check.
- **REQ-AGENT.BOARD-004** Every capability score shall be shown next to the same model's red-team results and the release's red-team verdict, and the board shall publish no single composite number that omits safety violations.
- **REQ-AGENT.BOARD-005** The claim register shall map each public capability claim to a board entry, and if a claim in the docs or website names no entry, then the claim check shall fail naming the claim.
- **REQ-AGENT.BOARD-006** The scoreboard shall be a static bundle viewable offline with no account and no tracking (C7).

## Interfaces

- **Offers:** the scoreboard bundle; the recompute check; the claim register and claim check consumed by `OUTR` and the release notes in `ECO`.
- **Consumes:** `AGENT.HARNESS` run records; `AGENT.REDTEAM` verdict.

## Decomposition hints

BUNDLE (board data and static page), RECOMP (recompute check), SUBMIT (third-party submissions), CLAIMS (claim register and check).

## Open questions

- Who pays for runs of third-party hosted models for the public board? Sponsorship may be needed; until then the board may list only open-weight models run in project CI and third-party submissions.
- Some model providers' terms may restrict publishing benchmark results [unverified]; maintainers should check before publishing scores of hosted models.
- D-032 sets whether the recompute check is a `fid agent` command or part of `fid-eval`.

## Children

_None yet: status `open`._
