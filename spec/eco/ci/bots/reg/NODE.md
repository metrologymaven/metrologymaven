---
id: ECO.CI.BOTS.REG
title: Bot register and restricted-path exclusion check
level: 4
parent: ECO.CI.BOTS
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-ECO.CI.BOTS-003, REQ-ECO.CI.BOTS-004]
covers: []
depends_on: []
uses_artifacts: [ECO.RULES.SRC]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.CI.BOTS.REG: Bot register and restricted-path exclusion check

## Purpose

One reviewed file, `bots.toml`, lists every automated reviewer the project uses, with its forge account, licence or terms, data-handling statement, review date and approving maintainer. A check keeps the register complete and current, and proves for every bot that sends repository content to an external service that its configuration excludes every restricted path. The same account list tells the PR-timeline tool which actors are bots.

## Scope

**In:**
- `bots.toml` schema and initial content (the self-hosted reviewer only)
- `cargo xtask bots-check`: register completeness, review age, exclusion coverage, canary record
- Reading exclusion lists out of bot configuration files (TOML, YAML or JSON) at a key path named in the register

**Out:**
- The restricted-path list (owned by `ECO.RULES.SRC`)
- The self-hosted reviewer (owned by `ECO.CI.BOTS.REVIEW`)
- Treating bot accounts in the PR clock (owned by `ECO.COMM.TIMELINE`, which reads `account` values)

## Requirements

- **REQ-ECO.CI.BOTS.REG-001** Each `[[bot]]` entry shall have `id`, `kind` (`self-hosted` or `hosted`), `account`, `terms` (SPDX ID or URL), `data_handling` (one line), `reviewed_on` (ISO date), `approved_by` (maintainer handle) and `sends_content` (bool), and the check shall fail with `BOTS-SCHEMA` on a missing or malformed field.
- **REQ-ECO.CI.BOTS.REG-002** If `reviewed_on` is more than 365 days before the `--today` input, then the check shall fail with `BOTS-STALE`.
- **REQ-ECO.CI.BOTS.REG-003** If an entry has `sends_content = true`, then the check shall fail with `BOTS-EXCLUDE` unless the exclusion list read from its `config` file at `exclude_key` contains a pattern equal to each line of `restricted-paths.txt`.
- **REQ-ECO.CI.BOTS.REG-004** If an entry has `sends_content = true` and no `canary_verified_on` date, then the check shall fail with `BOTS-CANARY`; the date records that a maintainer opened a canary PR touching a restricted canary file and found no excerpt in the service's request log.
- **REQ-ECO.CI.BOTS.REG-005** If an account posts review comments in the forge export given with `--review-export` and is marked as a bot by the forge but is not an `account` in the register, then the check shall fail with `BOTS-UNREGISTERED` naming the account.

## Acceptance criteria

- **AC-1** Given `fixtures/bots/ok/` (register with the self-hosted reviewer and one hosted bot whose YAML config excludes both lines of `restricted-paths.txt`, with a canary date), when `cargo xtask bots-check --register fixtures/bots/ok/bots.toml --restricted fixtures/bots/ok/restricted-paths.txt --today 2027-01-10` runs, then it exits 0.
- **AC-2** (error path) Given `fixtures/bots/missing-exclude/` (config lacks `standards/**`), then `BOTS-EXCLUDE hosted-x standards/**`.
- **AC-3** (error path) With `--today 2028-06-01` on AC-1, then `BOTS-STALE` for every entry reviewed before 2027-06-02.
- **AC-4** (error path) Given `fixtures/bots/no-canary/`, then `BOTS-CANARY hosted-x`; given `--review-export fixtures/bots/unregistered/reviews.json` with comments from bot account `lint-bot[bot]` absent from the register, then `BOTS-UNREGISTERED lint-bot[bot]`.
- **AC-5** (determinism) Output is sorted by code then bot ID and byte-identical across two runs.

## Design

- **Registry command(s):** none; repository tool `cargo xtask bots-check --register <toml> --restricted <txt> --today <date> [--review-export <json>]`.
- **Data structures:** `[[bot]] id, kind, account, terms, data_handling, reviewed_on, approved_by, sends_content, config?, exclude_key?, canary_verified_on?`; review export `[{author, author_is_bot}]` (exported by a workflow in ECO.CI.WF).
- **Algorithm:** bounded parse (64 KiB register, 256 KiB per config, 10 MiB export); key-path lookup in TOML, YAML or JSON chosen by file extension; exact string set comparison of patterns; set difference of bot accounts.
- **Errors:** `BOTS-SCHEMA`, `BOTS-STALE`, `BOTS-EXCLUDE`, `BOTS-CANARY`, `BOTS-UNREGISTERED`, `BOTS-IO`.
- **Determinism:** D0; the date is an input, never the clock.
- **Performance budget:** under 1 s [unverified until measured].
- **Crate:** `fid-xtask` (module `bots`).

## Test fixtures

Hand-written registers, bot configs, restricted lists and workflow files under `fixtures/bots/` (MIT OR Apache-2.0).

## Build prompt

> In `fid-xtask` (implementation repository), add `bots-check --register bots.toml --restricted house-rules/restricted-paths.txt --today <YYYY-MM-DD> [--review-export <json>]`. Register: `[[bot]]` with `id`, `kind` (`self-hosted|hosted`), `account`, `terms`, `data_handling`, `reviewed_on`, `approved_by`, `sends_content`, optional `config`, `exclude_key` (dotted key path), `canary_verified_on`. Findings sorted by code then id: `BOTS-SCHEMA`, `BOTS-STALE` (reviewed more than 365 days before `--today`), `BOTS-EXCLUDE` (for `sends_content = true`, the config's exclusion list at `exclude_key`, read as TOML/YAML/JSON by extension, must contain every restricted pattern verbatim), `BOTS-CANARY` (`sends_content = true` without `canary_verified_on`), `BOTS-UNREGISTERED` (an account in the review export flagged `author_is_bot` that is not a register `account`), `BOTS-IO`. Exit 1 on any finding. Seed `bots.toml` with the self-hosted reviewer (`kind = "self-hosted"`, `sends_content = false`). Tests for AC-1..AC-5 of spec node ECO.CI.BOTS.REG. Bounded inputs; no clock reads; no `unwrap`/`expect`/`panic`/`unsafe`.

## Open questions

- REQ-005 relies on the forge's own bot flag in the export; a bot posting through a person's account is not detected, so reviewers still watch CI PRs (which land alone, per ECO.CI.GATES).

## Children

_Leaf: none._
