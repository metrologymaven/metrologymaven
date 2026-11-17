---
id: ECO.RULES.CHECK
title: House-rules check (edited, stale, missing, budgets)
level: 3
parent: ECO.RULES
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.RULES-003, REQ-ECO.RULES-004, REQ-ECO.RULES-005, REQ-ECO.RULES-006, REQ-ECO.RULES-008]
covers: []
depends_on: [ECO.RULES.SRC, ECO.RULES.BLOCK]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.RULES.CHECK: House-rules check

## Purpose

The check every repository's CI calls: it fails when a generated block was edited, when a required block is missing or when `CLAUDE.md` is anything other than a reference to `AGENTS.md`; it warns when a block is behind the current canonical revision or when the size budgets are exceeded. It never writes files.

## Scope

**In:**
- `python -m house_rules check --target <dir> --repo-id <id> --manifest <file> --current-rev <rev> [--source-repo <dir>]`
- Exit codes: 0 ok (warnings allowed), 1 fail

**Out:**
- Repairing anything (owned by `ECO.RULES.SYNC`)
- Deciding when it runs (owned by `ECO.CI.GATES`)

## Requirements

- **REQ-ECO.RULES.CHECK-001** If a block's body does not match its recorded digest, then the check shall fail with `RULES-EDITED <file> <block>`.
- **REQ-ECO.RULES.CHECK-002** If a block assigned to the repository in the manifest is missing from `AGENTS.md`, then the check shall fail with `RULES-MISSING <block>`.
- **REQ-ECO.RULES.CHECK-003** If a block's recorded revision differs from `--current-rev`, then the check shall warn `RULES-STALE`; when `--source-repo` is given, it shall warn only if the recorded revision is an ancestor of the current one, and fail with `RULES-UNKNOWN-REV` if it is not in the source history.
- **REQ-ECO.RULES.CHECK-004** If the `core` block body exceeds the manifest's `core_bytes` or `AGENTS.md` exceeds `agents_md_bytes`, then the check shall warn and print the measured size.
- **REQ-ECO.RULES.CHECK-005** If `CLAUDE.md` is missing or its content is not exactly `@AGENTS.md` with an optional trailing newline, then the check shall fail with `RULES-CLAUDE-MD`.
- **REQ-ECO.RULES.CHECK-006** The check shall not modify any file.

## Acceptance criteria

- **AC-1** Given `fixtures/check/ok/` (current blocks, correct `CLAUDE.md`), when the check runs with `--current-rev` equal to the recorded revision, then it exits 0 with no output lines.
- **AC-2** (error path) Given `fixtures/check/edited/`, then it exits 1 with `RULES-EDITED AGENTS.md core`.
- **AC-3** (error path) Given `fixtures/check/missing/` (no `rust` block in a code repository), then it exits 1 with `RULES-MISSING rust`.
- **AC-4** Given `fixtures/check/ok/` and a different `--current-rev`, then it exits 0 and prints `warn RULES-STALE core`.
- **AC-5** Given `fixtures/check/big/` (an `AGENTS.md` of 11 000 bytes), then it exits 0 and prints `warn RULES-SIZE AGENTS.md 11000 > 10240`.
- **AC-6** (error path) Given `fixtures/check/claude-extra/` (`CLAUDE.md` with an extra line), then it exits 1 with `RULES-CLAUDE-MD`.
- **AC-7** A test hashes every fixture file before and after AC-1..AC-6 and finds no change.

## Design

- **Registry command(s):** none; repository tool.
- **Data structures:** manifest (ECO.RULES.SRC), `Block` (ECO.RULES.BLOCK).
- **Algorithm:** parse `AGENTS.md` blocks; compare with the manifest assignment for `--repo-id`; verify digests; compare revisions (with `git merge-base --is-ancestor` when `--source-repo` is given); measure sizes after LF normalisation; check `CLAUDE.md`.
- **Errors:** `RULES-EDITED`, `RULES-MISSING`, `RULES-UNKNOWN-REV`, `RULES-CLAUDE-MD`, `RULES-PARSE`; warnings `RULES-STALE`, `RULES-SIZE`.
- **Determinism:** D0.
- **Performance budget:** under 1 s.
- **Crate:** not Rust; Python package `tools/house_rules/` (module `check.py`), standard library only.

## Test fixtures

Fixture repositories under `tools/tests/fixtures/check/`, generated with `block.render` so digests are correct, then edited where a test needs it.

## Build prompt

> In `tools/house_rules/` (Python stdlib, uses `block.py`), add `check.py` and the `check` subcommand: `python -m house_rules check --target <dir> --repo-id <id> --manifest <file> --current-rev <40-hex> [--source-repo <dir>]`. Fail (exit 1) on `RULES-EDITED` (digest mismatch), `RULES-MISSING` (assigned block absent), `RULES-UNKNOWN-REV` (with `--source-repo`, recorded rev not in history), `RULES-CLAUDE-MD` (CLAUDE.md absent or not exactly `@AGENTS.md` plus optional newline), `RULES-PARSE`. Warn on `RULES-STALE` (rev differs; with `--source-repo`, only if ancestor) and `RULES-SIZE` (core body > core_bytes, AGENTS.md > agents_md_bytes). Never write files. Sorted output. Write `unittest` tests for AC-1..AC-7 of spec node ECO.RULES.CHECK.

## Open questions

- None.

## Children

_Leaf: none._
