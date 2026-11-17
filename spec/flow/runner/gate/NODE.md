---
id: FLOW.RUNNER.GATE
title: Pass rules and per-app GA gate evaluation
level: 3
parent: FLOW.RUNNER
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-FLOW.RUNNER-005, REQ-FLOW.RUNNER-006, REQ-FLOW.RUNNER-007, REQ-FLOW.RUNNER-011]
covers: []
depends_on: [FLOW.RUNNER.PKG, FLOW.RUNNER.VERDICT]
uses_artifacts: [FLOW.STUDY.RECORD]
side_effect: R1
determinism: D0
cli: [fid accept evaluate-gate]
mcp: [accept_evaluate_gate]
journal: [accept.evaluate_gate@1]
updated: 2026-11-17
---

# FLOW.RUNNER.GATE: Pass rules and per-app GA gate evaluation

## Purpose

Combines nightly verdicts with design-partner study aggregates into a gate status for one app and one milestone. Per D-017 an app's 1.0 needs every phase holding its core workflow finished; this command answers the narrower question the release process asks: do all gating packages of this app for this milestone pass both the machine verdict and the partner-time rule?

## Scope

**In:**
- `accept.evaluate_gate@1` (R1): inputs app ID and milestone ID
- Gating set: packages with `gating: gating`, `assistance: unassisted`, the given app and the milestone in `milestones`
- Pass rule per job: latest nightly pass not older than 7 days, and study median task time across at least 3 partners within target with no partner above 1.5 times the target
- Gate status document consumed by `ECO.REL`

**Out:**
- Nightly verdicts (owned by `FLOW.RUNNER.NIGHTLY`, `FLOW.RUNNER.VERDICT`)
- Study records and aggregates (owned by `FLOW.STUDY.RECORD`)
- Blocking a release (owned by `ECO.REL`)

## Requirements

- **REQ-FLOW.RUNNER.GATE-001** The gate shall consider only packages that are gating, unassisted, of the given app and list the given milestone, and shall list every other package of the app as excluded with its reason.
- **REQ-FLOW.RUNNER.GATE-002** A job shall pass only if its latest nightly verdict is `pass` on at least one reference machine and every gating variant of the job passes.
- **REQ-FLOW.RUNNER.GATE-003** A job shall pass the time rule only if the study aggregate for the job and release has at least 3 partners, its median task time is at most the package `task_s` target, and no partner's median exceeds 1.5 times that target.
- **REQ-FLOW.RUNNER.GATE-004** The gate status shall be `failed` while any gating job fails REQ-002 or REQ-003, has no nightly verdict newer than 7 days, or has study data from fewer than 3 partners, and shall name each cause.
- **REQ-FLOW.RUNNER.GATE-005** The command shall be R1, take no parameter that alters a package, target or flag, and return a status document with its RFC 8785 digest.

## Acceptance criteria

- **AC-1** Given fixture reports and study aggregate `fixtures/flow/gate/ok/` (2 gating jobs, 3 partners each within target), when `fid accept evaluate-gate --app report --milestone launch-1 --json` runs, then status is `passed`.
- **AC-2** (error path) Given `two-partners/`, then status is `failed` with cause `insufficient_partners`.
- **AC-3** (error path) Given `outlier/` (one partner at 1.6 times target), then `failed` with `partner_over_limit`.
- **AC-4** (error path) Given `stale/` (latest nightly 8 days old), then `failed` with `stale_nightly`.
- **AC-5** Given a tracked package failing, then status is unaffected and the package is listed as excluded.

## Design

- **Registry command(s):** `accept.evaluate_gate@1`, R1; params `app: AppId`, `milestone: string`, `as_of?: date`. Result `GateStatus { app, milestone, status: passed|failed, jobs:[{job, machine, time, causes}], excluded:[...], digest }`.
- **Algorithm:** select gating set; latest report per machine from the trend history; study aggregates for the release; apply rules; sort.
- **Errors:** `GATE-NO-PACKAGES` (no gating package: status `failed`), `GATE-STUDY-FORMAT`.
- **Determinism:** D0 given the same reports, aggregates and `as_of`.
- **Performance budget:** under 1 s [unverified].
- **Crate:** `fid-accept` module `gate`.

## Test fixtures

Synthetic reports and aggregates under `fixtures/flow/gate/`.

## Build prompt

> In crate `fid-accept`, add `accept.evaluate_gate@1` (R1; CLI `fid accept evaluate-gate --app <id> --milestone <id> [--as-of <date>]`; MCP `accept_evaluate_gate`). Gating set: packages with gating=gating, assistance=unassisted, matching app, milestone in `milestones`; list the rest as excluded. Per job: latest nightly verdict pass (not older than 7 days) for all gating variants; study aggregate for the release with >= 3 partners, median task time <= task_s, no partner median > 1.5 x task_s. Status failed with named causes (`job_failed`, `stale_nightly`, `insufficient_partners`, `median_over_target`, `partner_over_limit`, `no_packages`). Return a canonical status document with digest. No parameter may alter packages. Tests AC-1..AC-5 of spec node FLOW.RUNNER.GATE.

## Open questions

- Per D-017 (accepted, option A) Report and Inspect reach 1.0 only after P1b; earlier milestones are beta labelled by scope. The gate is evaluated per milestone, so a beta milestone can use the same rule with its own job list.

## Children

_Leaf: none._
