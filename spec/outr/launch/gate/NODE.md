---
id: OUTR.LAUNCH.GATE
title: Milestone gate check and report
level: 3
parent: OUTR.LAUNCH
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.LAUNCH-003, REQ-OUTR.LAUNCH-004]
covers: []
depends_on: [OUTR.LAUNCH.PLAN.FMT]
uses_artifacts: [ECO.PARITY.TRACE, ECO.PARITY.STATUS, ECO.CI.WF, ECO.COMM.SCOPE]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.LAUNCH.GATE: Milestone gate check and report

## Purpose

When a release candidate is tagged for a milestone, evaluates every criterion of that milestone from generated data only (the spec tree at the candidate's revision, the candidate's parity trace and status report, its CI test report, and committed sign-off records) and writes a gate report with an overall `ready` or `not_ready` result. The announcement check refuses to use a milestone's name unless the report says `ready`. Criteria are evaluated, never asserted.

## Scope

**In:** the `gate-check` subcommand; one evaluator per criterion kind of the criteria file; sign-off records and their validation against the roster; the gate report and its overall result.
**Out:** the criteria file format and content (`OUTR.LAUNCH.PLAN.FMT`, `OUTR.LAUNCH.PLAN.DOC`); parity trace, status report and CI test report (`ECO.PARITY.TRACE`, `ECO.PARITY.STATUS`, `ECO.CI.WF`); the roster file and roles (GOVERNANCE.md, D-003); posting announcements (`OUTR.LAUNCH.ANN`).

## Requirements

- **REQ-OUTR.LAUNCH.GATE-001** When `gate-check` runs for a milestone and a release candidate, it shall write a report listing every criterion of that milestone as `met`, `not_met` or `needs_signoff`, each with its evidence (node IDs and statuses, failing leaves, parity values and seed states, or the sign-off record).
- **REQ-OUTR.LAUNCH.GATE-002** The check shall read only the spec tree directory, the criteria file, the trace, status and CI test report files, the sign-off directory and the roster given on its command line, and shall make no network request.
- **REQ-OUTR.LAUNCH.GATE-003** A `node_status` criterion shall be met when the named node's `status` is in the criterion's list; a `phase_done` criterion shall be met when no node in the scope subtrees with a phase at or before the criterion's phase has status `open` or `decision`, and every leaf among them has at least one acceptance-criterion result and all its results are `pass` in the CI test report.
- **REQ-OUTR.LAUNCH.GATE-004** A `parity` criterion shall be met when the app's live percentage in the status report is at least `min_pct`, or when every listed seed has state `live` in the trace.
- **REQ-OUTR.LAUNCH.GATE-005** A `signoff` criterion shall be met only by a committed record for that milestone and criterion whose signer holds the named role in the roster, whose release equals the candidate, and whose signer is not the candidate's author; otherwise it shall be `needs_signoff` and the evidence shall say why.
- **REQ-OUTR.LAUNCH.GATE-006** The overall result shall be `ready` only when every criterion is `met`, and `not_ready` otherwise.
- **REQ-OUTR.LAUNCH.GATE-007** Two runs on the same inputs shall produce byte-identical reports.
- **REQ-OUTR.LAUNCH.GATE-008** The check shall exit 0 when the result is `ready`, 1 when it is `not_ready`, and 2 when an input cannot be read or the milestone is not in the criteria file.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/launch/gate/ready` (a small spec tree, a criteria file with one criterion of each kind for `inspect-beta`, trace, status and CI reports that satisfy them, a valid sign-off by a roster `release-manager` who is not the author), when `fid-outreach gate-check --milestone inspect-beta --release v0.5.0-beta.1 --author bob --criteria criteria.toml --tree spec --trace trace.json --status status.json --tests tests.json --signoffs signoffs --roster roster.toml --out out` runs, then it exits 0 and the report's result is `ready`.
- **AC-2** Given fixture `gate/open-node` (one node in a `phase_done` scope is `open`), when the check runs, then that criterion is `not_met` naming the node, and the result is `not_ready`.
- **AC-3** Given fixture `gate/failing-ac` (a leaf in scope has one failing AC result), when the check runs, then the `phase_done` criterion is `not_met` naming the leaf and AC.
- **AC-4** Given fixture `gate/seed-not-live`, when the check runs, then the `parity` criterion is `not_met` naming the seed.
- **AC-5** Given fixture `gate/self-signoff` (signer equals `--author`) and fixture `gate/wrong-role`, when the check runs, then the `signoff` criterion is `needs_signoff` with reason `self_signoff` or `role_missing`.
- **AC-6** (determinism) Given AC-1 run twice, then the reports are byte-identical.
- **AC-7** (error path) Given `--milestone ghost`, when the check runs, then it exits 2 and writes nothing.

## Design

- **Command:** `fid-outreach gate-check --milestone <id> --release <tag> --author <handle> --criteria <file> --tree <dir> --trace <file> --status <file> --tests <file> --signoffs <dir> --roster <file> --out <dir> [--json]`. Project tooling; nearest class W1 (writes `gate-<milestone>-<release>.json` and `.md` into a local directory).
- **Tree reading:** frontmatter `id`, `status`, `phase` of every `NODE.md` under `--tree`, with the same frontmatter rules as `tools/scope.py`; subtree = IDs with the scope ID as dotted prefix.
- **Inputs read:** trace `seeds[] {id, state}`; status `apps[] {id, live, in_scope}` (percentage computed in integers); CI test report `results[] {leaf, ac, outcome}`; sign-off records `signoffs/<milestone>/<criterion>.toml` with `milestone`, `criterion`, `release`, `signer`, `role`, `date`; roster `[[person]] handle, roles[]`.
- **Phase order:** P0 < P1a < P1b < P2 < P3 < P4 < later.
- **Errors:** fatal `InputUnreadable`, `InputParse`, `MilestoneUnknown`; per-criterion reasons `node_missing`, `status_mismatch`, `node_open`, `leaf_untested`, `ac_failed`, `below_threshold`, `seed_not_live`, `signoff_missing`, `role_missing`, `self_signoff`, `release_mismatch`.
- **Determinism:** D0. Sorted inputs, integer arithmetic, no clock.
- **Performance budget:** a 2,000-node tree and a 10,000-result test report in under 2 s [unverified until measured].
- **Crate:** `fid-outreach`, module `launch_gate` (reuses the `launch_plan` parser).

## Test fixtures

Synthetic trees, criteria files, trace, status and test reports, sign-off records and rosters under `fixtures/outr/launch/gate/`. CC-BY-4.0.

## Build prompt

> Build `fid-outreach gate-check --milestone <id> --release <tag> --author <handle> --criteria <file> --tree <dir> --trace <file> --status <file> --tests <file> --signoffs <dir> --roster <file> --out <dir> [--json]` in the Rust outreach tooling crate `fid-outreach`, reusing the crate's `launch_plan` criteria parser. Evaluate each criterion of the milestone: `node_status` (node's frontmatter `status` in list); `phase_done` (in the subtrees of `scope`, no node with phase at or before `phase` is `open` or `decision`, and every such leaf has at least one result in `tests.json` `results[] {leaf, ac, outcome}` and all are `pass`); `parity` (`apps[].live * 100 >= min_pct * in_scope` from `status.json`, or every listed seed `state == "live"` in `trace.json`); `signoff` (record `signoffs/<milestone>/<criterion>.toml` with `release` equal to `--release`, signer holding `role` in `roster.toml` `[[person]] handle, roles[]`, signer not equal to `--author`; else `needs_signoff` with reason). Read node frontmatter (`id`, `status`, `phase`) from every `NODE.md` under `--tree`. Write `gate-<milestone>-<release>.json` (RFC 8785) and `.md` with each criterion's state and evidence and an overall `ready` (all met) or `not_ready`. Exit 0 ready, 1 not ready, 2 unreadable input or unknown milestone (write nothing). No network, no clock.
>
> Acceptance tests under `fixtures/outr/launch/gate/` as listed in the spec.
>
> Constraints: no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded parsing; integer arithmetic for percentages; BTreeMap only; allowlisted dependency licences.

## Open questions

- The roster file with roles does not exist yet; GOVERNANCE.md (D-003) owns roles. OUTR.BRAND.REVIEW reads a plain handle list; both should converge on one roster file with roles, decided by the governance owners.
- The CI test report format is ECO.CI.WF's (`results[] {leaf, ac, outcome}`, as ECO.PARITY.TRACE reads it); only the input mapping changes if it differs.

## Children

_Leaf: none._
