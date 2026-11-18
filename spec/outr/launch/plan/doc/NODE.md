---
id: OUTR.LAUNCH.PLAN.DOC
title: Launch plan document and per-app milestone criteria
level: 4
parent: OUTR.LAUNCH.PLAN
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.LAUNCH.PLAN-001, REQ-OUTR.LAUNCH.PLAN-002]
covers: []
depends_on: [OUTR.LAUNCH.PLAN.FMT]
uses_artifacts: [ECO.APPS.LIST, ECO.PARITY.TRACE]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.LAUNCH.PLAN.DOC: Launch plan document and per-app milestone criteria

## Purpose

The written launch plan in the repository and the first content of its criteria file: for each focused app (Report, Inspect, Probe, Network), what alpha, beta and 1.0 mean and the checkable criteria for each, following D-017 (an app's 1.0 needs every phase holding its core workflow finished; earlier offline subsets ship as beta labelled by scope). The launch-1 milestones for Inspect and Report are tied to the launch-1 exit criteria.

## Scope

**In:** `launch/PLAN.md` (one section per app, plus a section on how milestones relate to phases); the initial `launch/criteria.toml` entries for all four apps.
**Out:** the file format and its check (`OUTR.LAUNCH.PLAN.FMT`); evaluation (`OUTR.LAUNCH.GATE`); announcements (`OUTR.LAUNCH.ANN`); reviews (`OUTR.LAUNCH.REVIEW`); phase definitions (CONSTITUTION §7).

## Requirements

- **REQ-OUTR.LAUNCH.PLAN.DOC-001** The plan shall have one section per app in the app list, each defining alpha, beta and 1.0 for that app in plain words, and one section explaining the D-017 rule.
- **REQ-OUTR.LAUNCH.PLAN.DOC-002** The criteria file shall contain an alpha, a beta and a 1.0 milestone for each of `report`, `inspect`, `probe` and `network`, each with at least one criterion.
- **REQ-OUTR.LAUNCH.PLAN.DOC-003** The `1.0` milestones of `report` and `inspect` shall include a `parity` criterion requiring seeds JOB.ACC.01, JOB.ACC.02, JOB.ACC.03 and JOB.ACC.05 to be live, matching the launch-1 exit criteria, and a `signoff` criterion for the role that confirms the correlation study is published.
- **REQ-OUTR.LAUNCH.PLAN.DOC-004** Every `beta` milestone of `probe` and `network` that precedes its core phase shall carry a `scope_label` naming the offline subset (for example "offline programming and simulation").
- **REQ-OUTR.LAUNCH.PLAN.DOC-005** Every percentage threshold in the criteria file shall carry a `note` marking it `[unverified]` until maintainers confirm it in review.
- **REQ-OUTR.LAUNCH.PLAN.DOC-006** `plan-check` shall pass on the plan and criteria file, and the plan shall pass the OUTR.BRAND claim lint.

## Acceptance criteria

- **AC-1** Given `launch/PLAN.md`, `launch/criteria.toml` and the repository's `apps.toml`, when `fid-outreach plan-check --criteria launch/criteria.toml --apps apps.toml --plan launch/PLAN.md --reviews launch/reviews` runs, then it exits 0.
- **AC-2** Given the criteria file, when a test lists milestones, then it finds 12 (four apps by three levels), each with at least one criterion.
- **AC-3** Given the `inspect` and `report` 1.0 milestones, when a test reads their `parity` criteria, then the seeds JOB.ACC.01, JOB.ACC.02, JOB.ACC.03 and JOB.ACC.05 are listed.
- **AC-4** Given the plan, when `fid-outreach claim-lint` runs on it, then it reports zero errors.

## Design

- **Registry command(s):** none; a document and a data file checked by `plan-check` and `claim-lint`.
- **Proposed milestone shape** (all thresholds `[unverified]`, set in review): alpha = the app's composition node and its P1a or P1b leaves at status leaf, plus a release-manager sign-off; beta = `parity` min_pct for the app plus the acceptance-job seeds of its workflow live, plus a sign-off; 1.0 = `phase_done` at the core phase over the app's composition node and the shared crates it uses, the acceptance-job seeds live, and the sign-offs.
- **Launch-1 mapping:** JOB.ACC.07 counts for Inspect only in its sections-and-thickness part; gap and flush moves to launch-2, so JOB.ACC.07 is not a 1.0 criterion of launch-1 milestones and is recorded as a `note`.
- **Roles:** sign-off roles name roster roles (for example `release-manager`, `maintainer`), never people.
- **Crate:** none (content).

## Test fixtures

The repository's own `apps.toml` and the files written by this leaf.

## Build prompt

> Write `launch/PLAN.md` and `launch/criteria.toml` for an open-source metrology project with four focused apps: report, inspect, probe, network. The rule (decision D-017): an app's 1.0 needs every phase that holds its core workflow finished (report and inspect after P1b, probe after P2, network after P3); earlier releases are alpha or beta, and offline subsets ship as beta labelled by scope. PLAN.md: one section on the rule, one section per app defining alpha, beta and 1.0 in plain words; no dates except those in the criteria file; no "certified" or "compliant" wording.
>
> criteria.toml (format checked by `fid-outreach plan-check`): 12 milestones (`<app>-alpha`, `<app>-beta`, `<app>-1-0`), each with criteria of kinds `node_status`, `phase_done`, `parity` (`app` + `min_pct`, or `seeds`), `signoff` (`role`). Report and inspect 1.0 must require seeds JOB.ACC.01, JOB.ACC.02, JOB.ACC.03, JOB.ACC.05 live and a sign-off that the correlation study is published; JOB.ACC.07 counts only for sections and thickness and goes in a `note`. Probe and network betas before their core phase need `scope_label`. Every percentage threshold has `note = "[unverified] to be confirmed by maintainers"`. Use roles, never names.
>
> Done when `plan-check` exits 0 and `claim-lint` reports zero errors on PLAN.md.

## Open questions

- Thresholds and roles are proposals for maintainers to confirm in the pull request (AGENTS rule 8).

## Children

_Leaf: none._
