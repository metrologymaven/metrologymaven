---
id: ECO.CI.WF.FORK
title: Fork-PR safety lint for workflows
level: 4
parent: ECO.CI.WF
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-ECO.CI.WF-004]
covers: []
depends_on: [ECO.CI.WF.CALL]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.CI.WF.FORK: Fork-PR safety lint for workflows

## Purpose

PRs from forks run untrusted code, so their jobs must have a read-only token and no secrets, and every required check must be able to pass that way. This leaf is a static lint over workflow files that enforces it: PR-triggered jobs declare read-only permissions and reference no secret; the privileged triggers that run with write access are allowed only in listed poster workflows that never check out PR code. It is the rule the self-hosted reviewer's poster and any later comment-posting job must obey.

## Scope

**In:**
- `cargo xtask ci-fork-lint` over `.github/workflows/*.yml`
- `fork-safe.toml`: the allowlist of privileged poster workflows and the permissions they may hold
- The rule that required checks come only from jobs triggered by `pull_request`

**Out:**
- Workflow content (owned by `ECO.CI.WF.CALL` and the bots' leaves)
- Required-check names (owned by `ECO.CI.WF.REQSET`)

## Requirements

- **REQ-ECO.CI.WF.FORK-001** If a workflow triggered by `pull_request` references `secrets.` other than `secrets.GITHUB_TOKEN`, or declares any `permissions` value other than `read` or `none` at workflow or job level, or omits a top-level `permissions` block, then the lint shall fail with `FORK-SECRET`, `FORK-PERM` or `FORK-NO-PERM`.
- **REQ-ECO.CI.WF.FORK-002** If a workflow uses the `pull_request_target` or `workflow_run` trigger and is not listed in `fork-safe.toml`, then the lint shall fail with `FORK-PRIV-TRIGGER`.
- **REQ-ECO.CI.WF.FORK-003** If a listed privileged workflow contains a checkout step whose `ref` refers to the PR head, or a `run` step that executes files from a downloaded artifact, then the lint shall fail with `FORK-PRIV-CHECKOUT` or `FORK-PRIV-EXEC`.
- **REQ-ECO.CI.WF.FORK-004** If a required check name in `required-checks.toml` maps to a job in a workflow not triggered by `pull_request`, then the lint shall fail with `FORK-REQ-TRIGGER`.

## Acceptance criteria

- **AC-1** Given `fixtures/fork/ok/` (a `pull_request` workflow with `permissions: {contents: read}` and a listed `workflow_run` poster that only downloads an artifact and calls the forge CLI), when `cargo xtask ci-fork-lint --root fixtures/fork/ok --required fixtures/fork/ok/required-checks.toml` runs, then it exits 0.
- **AC-2** (error path) Given `fixtures/fork/secret/` (a PR job using `secrets.DEPLOY_KEY`), then `FORK-SECRET`; given `fixtures/fork/write/` (`permissions: {contents: write}`), then `FORK-PERM`; given no permissions block, then `FORK-NO-PERM`.
- **AC-3** (error path) Given `fixtures/fork/target/` (an unlisted `pull_request_target` workflow), then `FORK-PRIV-TRIGGER`; given a listed poster checking out `github.event.pull_request.head.sha`, then `FORK-PRIV-CHECKOUT`.
- **AC-4** (error path) Given a required check served by a `push`-triggered job, then `FORK-REQ-TRIGGER`.
- **AC-5** (determinism) Findings are sorted by file and line and identical across runs.

## Design

- **Registry command(s):** none; repository tool `cargo xtask ci-fork-lint --root <repo> --required <toml> [--allow fork-safe.toml]`.
- **Data structures:** `fork-safe.toml`: `[[privileged]] file, permissions = {...}, reason`; parsed workflow subset: `on`, `permissions`, `jobs.*.permissions`, `jobs.*.steps[].uses|with.ref|run`.
- **Algorithm:** YAML subset parse (shared reader with ECO.CI.WF.CALL); rule checks; head-ref detection by matching `github.event.pull_request.head` and `github.head_ref` expressions; artifact-exec detection by flagging `run` steps that reference paths written by a download-artifact step.
- **Errors:** finding codes above plus `FORK-PARSE`, `FORK-IO`.
- **Determinism:** D0.
- **Performance budget:** under 1 s per repository [unverified until measured].
- **Crate:** `fid-xtask` (module `ci`).

## Test fixtures

Workflow trees under `fixtures/fork/` (MIT OR Apache-2.0).

## Build prompt

> In `fid-xtask` (implementation repository), add `ci-fork-lint --root <repo> --required required-checks.toml [--allow fork-safe.toml]`, reusing the YAML subset reader from `ci-pins`. Rules: `pull_request` workflows need a top-level `permissions` block with only `read`/`none` values (job-level too), and reference no `secrets.*` except `GITHUB_TOKEN` (`FORK-NO-PERM`, `FORK-PERM`, `FORK-SECRET`); `pull_request_target`/`workflow_run` workflows must be listed in `fork-safe.toml` (`FORK-PRIV-TRIGGER`), must not check out the PR head (`github.event.pull_request.head.*`, `github.head_ref`; `FORK-PRIV-CHECKOUT`) and must not execute files from downloaded artifacts (`FORK-PRIV-EXEC`); every required check must come from a `pull_request` job (`FORK-REQ-TRIGGER`). Sorted findings; exit 1 on any. Seed `fork-safe.toml` with the review poster from ECO.CI.BOTS.REVIEW. Tests for AC-1..AC-5 of spec node ECO.CI.WF.FORK. No `unwrap`/`expect`/`panic`/`unsafe`.

## Open questions

- Expression detection is syntactic; an indirect reference to the head ref through an intermediate variable may be missed. CI changes land alone (ECO.CI.GATES) so a reviewer sees them.

## Children

_Leaf: none._
