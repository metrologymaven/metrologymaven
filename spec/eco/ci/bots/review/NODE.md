---
id: ECO.CI.BOTS.REVIEW
title: Self-hosted reviewer and comment poster
level: 4
parent: ECO.CI.BOTS
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.CI.BOTS-001, REQ-ECO.CI.BOTS-003]
covers: []
depends_on: [ECO.CI.WF.FORK]
uses_artifacts: [ECO.CI.BOTS.REG]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.CI.BOTS.REVIEW: Self-hosted reviewer and comment poster

## Purpose

The automated reviewer that always runs, with no external service: an analysis job on the PR (read-only token, no secrets, fork-safe) runs advisory analysers on the changed lines and writes `review-findings.json`; a separate poster job, which never checks out PR code, turns those findings into review comments. Because nothing leaves the project's CI, restricted paths are never sent anywhere. Findings are advisory; a maintainer can promote one to blocking (ECO.CI.BOTS.BLOCK).

## Scope

**In:**
- `cargo xtask review-findings`: run advisory analysers, keep findings on changed lines, write `review-findings.json` v1
- The analyser set: the compiler linter's extended advisory groups, a dependency-duplication report and a TODO-without-issue scan (configurable in `review.toml`)
- Stable finding fingerprints so an updated PR does not repeat comments
- The poster workflow pattern: privileged job triggered after analysis, reading only the findings artifact

**Out:**
- Gating lints (owned by `ECO.CI.LINT`); this reviewer never fails a PR
- Blocking marks (owned by `ECO.CI.BOTS.BLOCK`)
- Fork token rules and the workflow lint that allows the poster (owned by `ECO.CI.WF.FORK`)
- The bot register (owned by `ECO.CI.BOTS.REG`, where this reviewer is the first entry)

## Requirements

- **REQ-ECO.CI.BOTS.REVIEW-001** When a PR is opened or updated, the analysis job shall write `review-findings.json` listing only findings on lines added or changed by the PR, each with `fingerprint`, `path`, `line`, `rule`, `severity` (`note` or `warning`) and `message`.
- **REQ-ECO.CI.BOTS.REVIEW-002** The poster job shall post each finding whose fingerprint is not already present in an earlier comment by the reviewer account as one review comment, and shall post nothing else.
- **REQ-ECO.CI.BOTS.REVIEW-003** The analysis job shall exit 0 whatever it finds, so its findings never block merging on their own.
- **REQ-ECO.CI.BOTS.REVIEW-004** The poster job shall not check out or execute code from the PR head and shall read only the findings artifact, whose size it shall cap at 1 MiB and whose schema it shall validate before posting.
- **REQ-ECO.CI.BOTS.REVIEW-005** The analysis job and poster job together shall finish within 10 minutes for a PR touching up to 50 files [unverified until measured], inside the ECO.CI time budget.
- **REQ-ECO.CI.BOTS.REVIEW-006** Neither job shall make a network request to any host other than the forge API and the CI's own artifact store.

## Acceptance criteria

- **AC-1** Given fixture repository `fixtures/review/repo/` and diff `fixtures/review/pr1.diff` (adds a function with a redundant clone on line 14 of `src/a.rs` and leaves an older warning on line 3 untouched), when `cargo xtask review-findings --repo fixtures/review/repo --diff fixtures/review/pr1.diff --out <tmp>` runs, then `review-findings.json` holds exactly one finding at `src/a.rs:14` and the exit code is 0.
- **AC-2** Given AC-1's findings and `fixtures/review/existing-comments.json` that already contains its fingerprint, when `cargo xtask review-post --findings <tmp>/review-findings.json --existing fixtures/review/existing-comments.json --dry-run` runs, then the planned post list is empty; with an empty existing list it has one comment.
- **AC-3** (error path) Given a 2 MiB findings file or one with an unknown field, when `review-post` runs, then it exits 1 with `REVIEW-ARTIFACT` and plans no posts.
- **AC-4** (fork safety) The workflow-lint fixture run of ECO.CI.WF.FORK on the shipped `review.yml` and `review-post.yml` reports no finding.
- **AC-5** (determinism) Two runs of AC-1 produce byte-identical output, and fingerprints do not change when unrelated lines above the finding move by 5 lines.

## Design

- **Registry command(s):** none; repository tools `cargo xtask review-findings --repo <dir> --diff <file> --out <dir>` and `cargo xtask review-post --findings <json> --existing <json> [--dry-run]` (posting itself is a thin workflow step using the forge CLI on the plan).
- **Data structures:** `review-findings.json`: `{schema: 1, revision, findings: [{fingerprint, path, line, rule, severity, message}]}` sorted by path, line, rule; fingerprint = SHA-256 of rule, path and the normalised text of the flagged line plus two lines of context.
- **Algorithm:** parse the unified diff into changed-line sets; run the analysers with machine-readable output; filter to changed lines; compute fingerprints; write. Poster: validate schema and size, subtract existing fingerprints (parsed from a hidden marker in earlier comments), emit the plan.
- **Errors:** `REVIEW-DIFF`, `REVIEW-ANALYSER` (an analyser failed to run: recorded as a `note` finding, not a failure), `REVIEW-ARTIFACT`.
- **Determinism:** D0 for a given toolchain and inputs.
- **Performance budget:** REQ-005.
- **Crate:** `fid-xtask` (module `review`); workflows `review.yml` and `review-post.yml` in the shared workflow repository.

## Test fixtures

A miniature repository, diffs and existing-comment exports under `fixtures/review/` (MIT OR Apache-2.0).

## Build prompt

> In `fid-xtask` (implementation repository), add `review-findings --repo <dir> --diff <file> --out <dir>` and `review-post --findings <json> --existing <json> [--dry-run]`. `review-findings` runs advisory analysers configured in `review.toml` (default: the Rust linter's pedantic and nursery groups as warnings, duplicate-dependency report, TODO-without-issue scan), keeps only findings on lines added or changed in the diff, and writes `review-findings.json` `{schema: 1, revision, findings: [{fingerprint, path, line, rule, severity: note|warning, message}]}` sorted, with fingerprint = SHA-256(rule, path, normalised flagged line plus two context lines); always exit 0 except for unreadable input. `review-post` validates the artifact (max 1 MiB, strict schema; else `REVIEW-ARTIFACT`), removes fingerprints already present in the reviewer's earlier comments (hidden marker), and outputs the post plan; a thin workflow step posts it. Ship `review.yml` (pull_request, read-only token, no secrets, uploads the artifact) and `review-post.yml` (triggered after `review.yml` completes, never checks out PR code, downloads only the artifact, posts with the forge CLI). No network except the forge API and artifact store. Tests for AC-1..AC-5 of spec node ECO.CI.BOTS.REVIEW. No `unwrap`/`expect`/`panic`/`unsafe`.

## Open questions

- The privileged poster pattern assumes GitHub Actions' `workflow_run` trigger [unverified for other forges]; ECO.CI.WF records the forge assumption.

## Children

_Leaf: none._
