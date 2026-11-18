---
id: OUTR.COMM.MOD
title: Moderation procedure and moderation log
level: 3
parent: OUTR.COMM
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-outreach]
satisfies: [REQ-OUTR.COMM-003]
covers: []
depends_on: []
uses_artifacts: []
side_effect: [R0, W1]
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.COMM.MOD: Moderation procedure and moderation log

## Purpose

How the code of conduct is enforced on official channels: how reports are filed and acknowledged, who the moderators are, how conflicts of interest and escalation work, and the private moderation log that the steering group can audit. The procedure is a document; the log has a fixed format with two small tools, one that checks it (fields, conflicts, retention) and one that turns it into the quarterly counts the public report shows, with no case details.

## Scope

**In:**
- `community/moderation/PROCEDURE.md`: intake routes (one per channel plus one private, channel-independent route), acknowledgement target, outcome categories (no action, warning, removal, ban), conflict-of-interest rule, escalation to the steering group, log access and retention, all in our own words
- `community/moderation/RULES.md`: per-channel moderation rules text, linked from the channel inventory
- `community/moderation/roster.toml`: public moderator roster (handles only)
- The private log format and the `mod-check` and `mod-stats` commands

**Out** (and which node owns it instead):
- The code-of-conduct text and governance (GOVERNANCE.md, D-003)
- Channel inventory and listing (owned by `OUTR.COMM.CHAN`)
- Spam filtering inside a provider (provider configuration, recorded by `OUTR.COMM.CHAN`)
- Publishing the counts (owned by `OUTR.COMM.REPORT`)

## Requirements

- **REQ-OUTR.COMM.MOD-001** The procedure shall require that a moderator acknowledge every report filed through an official route within 72 hours [unverified target], and `mod-stats` shall compute the share of a quarter's reports acknowledged within that target.
- **REQ-OUTR.COMM.MOD-002** Each log entry shall record the case ID, date filed, date acknowledged, channel ID, outcome category (`no_action`, `warning`, `removal` or `ban`), handling moderator handle, `parties_conflicted` (moderator handles with a conflict) and `handover` (from and to handles, when one happened), and `mod-check` shall fail on any missing field or unknown category.
- **REQ-OUTR.COMM.MOD-003** The log shall be kept outside every public repository and readable by every steering-group member, and if the log path given to either command lies inside a public repository checkout, then the command shall fail with `log_in_repo` and read nothing.
- **REQ-OUTR.COMM.MOD-004** If an entry's handling moderator appears in its `parties_conflicted`, or a conflicted moderator handed over without a `handover` record, then `mod-check` shall fail with `conflict_not_handed_over` naming the case ID.
- **REQ-OUTR.COMM.MOD-005** For a quarter, `mod-stats` shall write only the number of reports per outcome category, the total, and the share acknowledged within the target, and shall write no case ID, date, channel or handle.
- **REQ-OUTR.COMM.MOD-006** If an entry is older than the retention period in the procedure (initially 24 months [unverified]) and is not anonymised, then `mod-check` shall fail with `retention_overdue` naming the case ID; anonymised entries keep only quarter, outcome category and whether the acknowledgement target was met.
- **REQ-OUTR.COMM.MOD-007** If a handling moderator is not on the roster, then `mod-check` shall fail with `moderator_unknown`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/comm/mod/ok/` (a log of five 2027Q1 entries, one handed over, in a temp folder outside the repository, and a roster), when `fid-outreach mod-check --log <tmp>/log.toml --roster roster.toml --today 2027-04-02 --json` runs, then it exits 0.
- **AC-2** Given the same log, when `fid-outreach mod-stats --log <tmp>/log.toml --quarter 2027Q1 --target-hours 72 --out counts.json` runs, then `counts.json` holds counts per category summing to 5 and the share acknowledged within 72 hours (4 of 5), and contains no case ID, date, channel or handle.
- **AC-3** (error path) Given `conflict/` (the handling moderator is listed as conflicted), when `mod-check` runs, then it exits 1 with `conflict_not_handed_over`.
- **AC-4** (error path) Given `old/` (an unanonymised entry filed 2024-12-01) and `--today 2027-04-02`, when `mod-check` runs, then it exits 1 with `retention_overdue`.
- **AC-5** (error path) Given `--log` inside the repository checkout, when either command runs, then it exits 2 with `log_in_repo`.
- **AC-6** (error path) Given `bad-category/` (outcome `suspension`), when `mod-check` runs, then it exits 1 with `field_invalid outcome`.
- **AC-7** (determinism) Given the same log and quarter, when `mod-stats` runs twice, then the outputs are byte-identical.

## Design

- **Commands:** `fid-outreach mod-check --log <file> --roster <file> --today <date> [--repo-root <dir>] [--json]` (nearest class R0) and `fid-outreach mod-stats --log <file> --quarter <YYYYQn> --target-hours <n> --out <file> [--repo-root <dir>]` (nearest class W1, writes one file). Recording cases is a human act by moderators; no command writes the log.
- **Log format (private TOML):** `[[case]] id, filed_at, acknowledged_at, channel, outcome, moderator, parties_conflicted = [], handover = { from, to }?`; anonymised form `[[anon]] quarter, outcome, ack_within_target`.
- **Counts output:** `{ "quarter", "total", "by_outcome": { ... }, "ack_within_target": { "count", "total", "target_hours" } }`, RFC 8785 JSON.
- **Errors:** `field_missing`, `field_invalid`, `conflict_not_handed_over`, `retention_overdue`, `moderator_unknown`, `log_in_repo`, `log_parse`.
- **Determinism:** D0 (dates from the log and `--today`; no clock reads).
- **Crate:** `fid-outreach`, module `moderation`; documents in the outreach repository under `community/moderation/`.

## Test fixtures

Fictional logs and rosters under `fixtures/outr/comm/mod/` (`ok`, `conflict`, `old`, `bad-category`), copied by tests into a temp folder outside the repository; CC-BY-4.0; all handles and cases are invented.

## Build prompt

> In the outreach repository, write in your own words `community/moderation/PROCEDURE.md` (intake routes: one per official channel and one private route independent of any channel; acknowledgement within 72 hours; outcome categories no action, warning, removal, ban; a moderator with a conflict of interest hands the case to another and the hand-over is logged; escalation to the steering group; the log is private, kept outside public repositories, readable by every steering-group member; entries are anonymised after 24 months), `community/moderation/RULES.md` (per-channel rules, referencing the code of conduct in GOVERNANCE.md rather than restating it) and `community/moderation/roster.toml` (moderator handles).
>
> In the Rust crate `fid-outreach`, add module `moderation` with `fid-outreach mod-check --log <file> --roster <file> --today <date> [--repo-root <dir>] [--json]` and `fid-outreach mod-stats --log <file> --quarter <YYYYQn> --target-hours <n> --out <file> [--repo-root <dir>]`. Log TOML: `[[case]]` with `id`, `filed_at`, `acknowledged_at`, `channel`, `outcome` (no_action | warning | removal | ban), `moderator`, `parties_conflicted`, optional `handover = { from, to }`; anonymised `[[anon]]` with `quarter`, `outcome`, `ack_within_target`. Checks: missing or invalid fields; `conflict_not_handed_over` when the handling moderator is conflicted; `moderator_unknown`; `retention_overdue` for unanonymised cases filed more than 24 months before `--today`; both commands refuse (`log_in_repo`, exit 2) a log path inside `--repo-root` (default: git top level). Stats: per-outcome counts, total, and count acknowledged within `--target-hours`, with no IDs, dates, channels or handles in the output.
>
> Tests with fixtures under `fixtures/outr/comm/mod/` (copied to a temp folder outside the repo) for: valid log, stats content and privacy, conflict, retention overdue, log inside repo, invalid category, byte-identical stats.
>
> Constraints: no panics, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; never print case details beyond case IDs in `mod-check` findings; no network; BTreeMap rather than HashMap.

## Open questions

- The 72-hour target and the 24-month retention period are proposals for maintainers to confirm.
- Where the private log is stored (which tool, which access control) must not leak data to third parties; it is a maintainer choice, and the tools only need a readable file outside public repositories.

## Children

_Leaf: none._
