---
id: OUTR.LAUNCH.PLAN.FMT
title: Milestone criteria file format and plan check
level: 4
parent: OUTR.LAUNCH.PLAN
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.LAUNCH.PLAN-002, REQ-OUTR.LAUNCH.PLAN-003, REQ-OUTR.LAUNCH.PLAN-004, REQ-OUTR.LAUNCH.PLAN-005]
covers: []
depends_on: []
uses_artifacts: [ECO.APPS.LIST]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.LAUNCH.PLAN.FMT: Milestone criteria file format and plan check

## Purpose

Defines `launch/criteria.toml`, the machine-readable file that holds every milestone of every focused app, its checkable criteria with stable IDs, its target date and, once announced, its announcement record; and the `plan-check` command that validates the file, enforces the D-017 milestone rule, keeps dates out of the plan text, and refuses silent changes to the criteria of an announced milestone. The gate check, the roadmap page and the post-launch reviews all read this one file.

## Scope

**In:** the criteria file schema; the five criterion forms; the D-017 rule table; the `plan-check` subcommand with its base-revision comparison; the date rule for the plan document.
**Out:** the plan text and the criteria values (`OUTR.LAUNCH.PLAN.DOC`); evaluating criteria (`OUTR.LAUNCH.GATE`); review entries (`OUTR.LAUNCH.REVIEW`); roadmap rendering (`OUTR.SITE.STATUS`).

## Requirements

- **REQ-OUTR.LAUNCH.PLAN.FMT-001** The criteria file shall hold `[[milestone]]` entries with `id`, `app` (an ID from the app list), `level` (`alpha`, `beta` or `1.0`), optional `scope_label`, optional `target_date`, optional `announced` (`date` and `release`), and `[[milestone.criterion]]` entries with a unique `id` and a `kind`.
- **REQ-OUTR.LAUNCH.PLAN.FMT-002** The check shall accept exactly these criterion kinds: `node_status` (`node`, `in`: list of statuses), `phase_done` (`phase`, `scope`: list of node IDs), `parity` (`app` with `min_pct`, or `seeds`: list of seed IDs that must be live), and `signoff` (`role`), and shall report `unknown_kind` for any other.
- **REQ-OUTR.LAUNCH.PLAN.FMT-003** If a criterion ID or milestone ID is not unique in the file, or does not match `[A-Z][A-Z0-9-]{2,31}` for criteria and `[a-z0-9-]{3,32}` for milestones, then the check shall report an error naming it.
- **REQ-OUTR.LAUNCH.PLAN.FMT-004** If a `1.0` milestone lacks a `phase_done` criterion whose phase is at or after the app's core phase in the rule table (report P1b, inspect P1b, probe P2, network P3, from CONSTITUTION §7 and D-017), then the check shall report `core_phase_missing`.
- **REQ-OUTR.LAUNCH.PLAN.FMT-005** If a `beta` milestone of an app has no `scope_label`, then the check shall report `scope_label_missing`.
- **REQ-OUTR.LAUNCH.PLAN.FMT-006** If the plan document contains an ISO 8601 date that is not a `target_date` or `announced.date` in the criteria file, then the check shall report `stray_date` with file and line.
- **REQ-OUTR.LAUNCH.PLAN.FMT-007** When run with `--base <file>`, if any criterion of a milestone that has `announced` in the base file was added, removed or changed, and no review entry under `launch/reviews/` lists that criterion ID in its `changes`, then the check shall report `announced_criteria_changed`.
- **REQ-OUTR.LAUNCH.PLAN.FMT-008** The check shall exit 0 with no findings, 1 with any finding, and 2 when an input cannot be read or parsed.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/launch/plan/ok` (criteria for `report` and `inspect` alpha, beta and 1.0, app list, plan document), when `fid-outreach plan-check --criteria criteria.toml --apps apps.toml --plan PLAN.md --reviews reviews --json` runs, then it exits 0.
- **AC-2** Given fixture `plan/bad-kind` (a criterion with `kind = "vibe"`), when the check runs, then it reports `unknown_kind`.
- **AC-3** Given fixture `plan/early-1.0` (probe 1.0 with `phase_done` at P1b only), when the check runs, then it reports `core_phase_missing`.
- **AC-4** Given fixture `plan/beta-unlabelled`, when the check runs, then it reports `scope_label_missing`.
- **AC-5** Given fixture `plan/stray-date` (PLAN.md mentions `2027-03-01`, not in the file), when the check runs, then it reports `stray_date` with the line.
- **AC-6** Given fixture `plan/changed-announced` (base file with `report-alpha` announced; new file changes `RPT-A-02`; no review entry), when the check runs with `--base`, then it reports `announced_criteria_changed` for `RPT-A-02`; with a review entry listing `RPT-A-02` in `changes`, it reports nothing.
- **AC-7** Given fixture `plan/dup-id`, when the check runs, then it reports `duplicate_id`.

## Design

- **Command:** `fid-outreach plan-check --criteria <file> --apps <apps.toml> --plan <file> --reviews <dir> [--base <file>] [--json]`. Project tooling; nearest class R0.
- **Schema:** `version = 1`; milestones and criteria as in REQ-001 and REQ-002; every criterion may carry `note` (free text, not evaluated). Dates are `YYYY-MM-DD`.
- **Rule table:** a constant in the module, `report = P1b`, `inspect = P1b`, `probe = P2`, `network = P3`, with a comment citing CONSTITUTION §7 and D-017; a new app needs a code change and a decision.
- **Review entries:** Markdown files with front matter `milestone`, `announced`, `changes` (list of criterion IDs); only the front matter is read here.
- **Errors:** findings `unknown_kind`, `bad_field`, `duplicate_id`, `bad_id`, `unknown_app`, `core_phase_missing`, `scope_label_missing`, `stray_date`, `announced_criteria_changed`; fatal `InputUnreadable`, `InputParse`.
- **Determinism:** D0.
- **Crate:** `fid-outreach`, module `launch_plan` (its parser is reused by the gate and review checks).

## Test fixtures

Synthetic criteria files, app lists, plan documents and review entries under `fixtures/outr/launch/plan/`. CC-BY-4.0.

## Build prompt

> Build the `launch_plan` module and `fid-outreach plan-check --criteria <file> --apps <apps.toml> --plan <file> --reviews <dir> [--base <file>] [--json]` in the Rust outreach tooling crate `fid-outreach`. Expose the parsed criteria types publicly; the gate and review checks reuse them.
>
> Schema (`criteria.toml`, `version = 1`): `[[milestone]] id ([a-z0-9-]{3,32}), app (must be in apps.toml [[app]] id), level (alpha | beta | 1.0), scope_label?, target_date?, announced? {date, release}`; `[[milestone.criterion]] id ([A-Z][A-Z0-9-]{2,31}, unique in file), kind, note?` with kinds `node_status {node, in[]}`, `phase_done {phase, scope[]}`, `parity {app, min_pct} | {seeds[]}`, `signoff {role}`; anything else is `unknown_kind`.
>
> Rules: a `1.0` milestone needs a `phase_done` with phase at or after the app's core phase (constant table: report P1b, inspect P1b, probe P2, network P3; cite CONSTITUTION §7 and D-017 in a comment) else `core_phase_missing`; a `beta` needs `scope_label`; every `YYYY-MM-DD` in the plan document must be a `target_date` or `announced.date` (`stray_date` with line); with `--base`, any added, removed or changed criterion of a milestone announced in the base file needs a review entry (Markdown front matter `milestone`, `announced`, `changes[]`) listing its ID, else `announced_criteria_changed`. Exit 0/1/2; RFC 8785 JSON report sorted by code then ID.
>
> Acceptance tests under `fixtures/outr/launch/plan/` as listed in the spec.
>
> Constraints: no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded parsing (64 KiB per file); no network; BTreeMap only; allowlisted dependency licences.

## Open questions

- REQ-OUTR.LAUNCH.PLAN-005 also allows a change through a pull request approved by another maintainer; that half is enforced by repository branch protection (ECO), not by this check.
- The roadmap check in OUTR.SITE.STATUS reads `target_date` and `announced.date` from this file.

## Children

_Leaf: none._
