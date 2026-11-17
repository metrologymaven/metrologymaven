---
id: OUTR.LAUNCH.ANN
title: Announcement plan and drafts
level: 3
parent: OUTR.LAUNCH
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.LAUNCH-004, REQ-OUTR.LAUNCH-005]
covers: []
depends_on: [OUTR.BRAND.LINT, OUTR.LAUNCH.PLAN.FMT]
uses_artifacts: [OUTR.LAUNCH.GATE, OUTR.COMM.CHAN, OUTR.SITE.DL, OUTR.DOCS.FRAME]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.LAUNCH.ANN: Announcement plan and drafts

## Purpose

The announcement side of a launch: an announcement plan file naming, per milestone, every channel, the draft to post there and the role that owns the post; the drafts themselves, kept in the repository; and a pre-posting check that a person runs before posting. The check passes only when the milestone's gate report says `ready`, every draft passes the claim lint, every channel is an official one, and every draft links the release's published download entry and docs edition. Posting is done by people; nothing here posts.

## Scope

**In:** `launch/announce/<milestone>/plan.toml`; drafts under `launch/announce/<milestone>/`; the `ann-check` subcommand; the posting runbook paragraph that tells the owner to run the check and post by hand.
**Out:** gate evaluation (`OUTR.LAUNCH.GATE`); the channel inventory (`OUTR.COMM.CHAN`); the download page and docs editions (`OUTR.SITE.DL`, `OUTR.DOCS.FRAME`); tutorials and videos linked from drafts (`OUTR.CONTENT`); paid promotion (business documents outside this tree).

## Requirements

- **REQ-OUTR.LAUNCH.ANN-001** Each milestone's announcement plan shall list entries with `channel` (an ID from the channel inventory), `draft` (a file path in the same folder) and `owner` (a roster role), and the check shall report `unknown_channel` for any channel not in the inventory and `draft_missing` for any missing draft.
- **REQ-OUTR.LAUNCH.ANN-002** The check shall run the OUTR.BRAND claim lint on every draft of the plan and report its errors with rule IDs.
- **REQ-OUTR.LAUNCH.ANN-003** If the gate report for the milestone and release is missing or its result is not `ready`, then the check shall report `gate_not_ready` for every draft that contains the milestone's display name (`<App> <level>`, case-insensitive).
- **REQ-OUTR.LAUNCH.ANN-004** If a draft does not link the release's download page anchor and the release's docs edition, or either target is absent from the given published site and docs trees, then the check shall report `link_missing` or `target_unpublished` naming the draft and link.
- **REQ-OUTR.LAUNCH.ANN-005** The check shall never send, post or upload anything, and the runbook shall state that a person posts each draft by hand after the check passes; an agent may write drafts but shall not post them (posting is X1 egress under CONSTITUTION §3).
- **REQ-OUTR.LAUNCH.ANN-006** The check shall exit 0 with no findings, 1 with any finding, and 2 when an input cannot be read.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/launch/ann/ok` (plan with two channels from the inventory, two clean drafts linking `download.html#v0-5-0-beta-1` and `docs/v0.5.0-beta.1/index.html`, a `ready` gate report, published site and docs trees containing both targets), when `fid-outreach ann-check --milestone inspect-beta --release v0.5.0-beta.1 --plan plan.toml --channels channels.toml --gate gate.json --criteria criteria.toml --site site --docs docs --rules claim-rules.toml` runs, then it exits 0.
- **AC-2** Given fixture `ann/not-ready` (gate result `not_ready`; a draft saying "Inspect beta is here"), when the check runs, then it reports `gate_not_ready` for that draft.
- **AC-3** Given fixture `ann/lint` (a draft saying "faster than" an incumbent with no benchmark link), when the check runs, then it reports claim-lint rule CL002.
- **AC-4** Given fixture `ann/unpublished` (docs tree lacks the edition), when the check runs, then it reports `target_unpublished`.
- **AC-5** Given fixture `ann/bad-channel`, when the check runs, then it reports `unknown_channel`.
- **AC-6** Given the crate source, when a test searches the `ann_check` module, then it contains no network client use.

## Design

- **Command:** `fid-outreach ann-check --milestone <id> --release <tag> --plan <file> --channels <file> --gate <file> --criteria <file> --site <dir> --docs <dir> --rules <file> [--json]`. Project tooling; nearest class R0. There is no posting command (REQ-005).
- **Plan file:** `version`, `milestone`, `[[post]] channel, draft, owner, timing` (`timing` is relative, for example `release+0d`, never a calendar date).
- **Display name:** app name from the criteria file's `app` (capitalised) plus `level` (`alpha`, `beta`, `1.0`).
- **Link targets:** download anchor `download.html#<release with dots replaced by hyphens>` in `--site` [unverified: anchor scheme fixed by OUTR.SITE.DL]; docs edition `<release>/index.html` in `--docs`.
- **Inputs read:** channel inventory IDs (OUTR.COMM.CHAN); gate report `milestone`, `release`, `result`.
- **Errors:** findings `unknown_channel`, `draft_missing`, `gate_not_ready`, `link_missing`, `target_unpublished`, plus claim-lint findings; fatal `InputUnreadable`.
- **Determinism:** D0.
- **Crate:** `fid-outreach`, module `ann_check`.

## Test fixtures

Synthetic plans, drafts, channel inventories, gate reports and published trees under `fixtures/outr/launch/ann/`. CC-BY-4.0.

## Build prompt

> Build `fid-outreach ann-check --milestone <id> --release <tag> --plan <file> --channels <file> --gate <file> --criteria <file> --site <dir> --docs <dir> --rules <file> [--json]` in the Rust outreach tooling crate `fid-outreach`, reusing its `claim_lint` and `launch_plan` modules. Plan file: `version`, `milestone`, `[[post]] channel, draft, owner, timing`. Checks: each `channel` is an ID in the channel inventory (`unknown_channel`); each `draft` exists (`draft_missing`); claim lint on every draft; if the gate report (`milestone`, `release`, `result`) is missing, for another release, or not `ready`, report `gate_not_ready` for each draft containing the milestone display name (`<App> <level>`, case-insensitive); each draft must link the download anchor (`download.html#<release, dots to hyphens>`) and the docs edition (`<release>/index.html`) (`link_missing`), and both targets must exist in `--site` and `--docs` (`target_unpublished`). Exit 0/1/2; RFC 8785 JSON report sorted by draft, code. The module must contain no network client and no posting feature. Also write `launch/announce/RUNBOOK.md`: run the check, then a person posts each draft by hand; agents may draft but never post.
>
> Acceptance tests under `fixtures/outr/launch/ann/` as listed in the spec.
>
> Constraints: no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded reads; no network; BTreeMap only; allowlisted dependency licences.

## Open questions

- The download anchor scheme belongs to OUTR.SITE.DL (still open); only the link pattern here changes if it differs.
- "Post only after the download entry and docs edition are published" is enforced by the published-target check (REQ-OUTR.LAUNCH.ANN-004), not by a clock.

## Children

_Leaf: none._
