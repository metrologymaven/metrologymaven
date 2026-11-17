---
id: OUTR.DOCS.CONTRIB
title: Contributor guide with checked commands
level: 3
parent: OUTR.DOCS
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.DOCS-006]
covers: []
depends_on: []
side_effect: R0
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.DOCS.CONTRIB: Contributor guide with checked commands

## Purpose

The contributor guide in the docs portal: how to find an open spec node, claim it, expand it with the expand-node prompt, and validate the tree, plus the house rules and a pointer to building from source. Every shell command the guide shows is marked runnable and executed by a small checker against a throwaway copy of the repository revision the guide documents, so the guide cannot drift from the tooling. This is a project deliverable (text plus a checker), not a product registry command.

## Scope

**In:** the guide pages (find, claim, expand, validate, house rules summary, building from source pointer); the runnable-block convention; the `doc-run` checker that executes runnable blocks in a temporary copy of the repository.
**Out:** the spec-tree tooling itself (`tools/scope.py`, owned by `ECO.COMM`); governance and code-of-conduct text (GOVERNANCE.md, D-003); house-rule sync (`ECO.RULES`); build-from-source instructions of the implementation repository (`ECO.REPO`; this guide links them once they exist and says plainly that they do not yet exist until then); portal frame (`OUTR.DOCS.FRAME`).

## Requirements

- **REQ-OUTR.DOCS.CONTRIB-001** The contributor guide shall have one section each on finding an open node, claiming it, expanding it, and validating the tree, and each section shall name the repository file or command it relies on (`tools/scope.py`, `prompts/expand-node.md`, `AGENTS.md`).
- **REQ-OUTR.DOCS.CONTRIB-002** The guide shall mark every command meant to be run with a fenced code block whose info string is `sh run`, and every such block shall be executed by the checker.
- **REQ-OUTR.DOCS.CONTRIB-003** When the checker runs, it shall copy the repository at the documented revision to a temporary directory, run each runnable block there in document order, and fail if any block exits non-zero, naming the guide file and block line.
- **REQ-OUTR.DOCS.CONTRIB-004** The checker shall never modify the working repository it was started from.
- **REQ-OUTR.DOCS.CONTRIB-005** The guide shall state the repository revision it documents, and the checker shall fail if that revision is not the revision being checked.
- **REQ-OUTR.DOCS.CONTRIB-006** The guide shall pass the OUTR.BRAND claim lint with zero errors.

## Acceptance criteria

- **AC-1** Given this spec repository at revision R and the guide stating revision R, when `fid-outreach doc-run --guide docs/contributor/guide.md --repo . --json` runs, then every `sh run` block (at least `python tools/scope.py open`, `python tools/scope.py check` and `python tools/scope.py stats`) exits 0 and the checker exits 0.
- **AC-2** (error path) Given fixture `fixtures/outr/contrib/broken-cmd` (a guide with a `sh run` block calling `python tools/scope.py no-such-verb`), when the checker runs, then it exits 1 and names the guide file and the block's line number.
- **AC-3** (isolation) Given the AC-1 run, when it finishes, then `git status --porcelain` in the starting repository is unchanged from before the run (the `generated/` reports written by `scope.py check` land only in the temporary copy).
- **AC-4** (revision guard) Given fixture `fixtures/outr/contrib/wrong-rev` (guide states a revision different from HEAD), when the checker runs, then it exits 1 with error `revision_mismatch` before running any block.
- **AC-5** (structure) Given the guide, when a test parses its headings, then it finds the four sections Find, Claim, Expand and Validate, and each names at least one of `tools/scope.py`, `prompts/expand-node.md` or `AGENTS.md`.
- **AC-6** Given the guide, when the OUTR.BRAND claim lint runs on it, then it reports zero errors.

## Design

- **Command:** `fid-outreach doc-run --guide <file> --repo <dir> [--json]`. Project tooling, not a registry command; nearest class R0 because the user's repository is only read (writes happen in a temporary copy that is deleted afterwards).
- **Runnable block convention:** fenced block with info string `sh run`; one command per line; blocks run with the temporary repository root as working directory; a block may declare `sh run expect-fail` for documented error examples, which must then exit non-zero.
- **Algorithm:** read the guide; extract `revision:` from its front matter; compare with `git rev-parse HEAD` of `--repo`; export that revision (`git worktree add` or archive) to a temporary directory; run blocks in order with a per-block timeout (60 s default); collect exit codes and stderr tails; delete the temporary directory; write the report.
- **Errors:** `RevisionMismatch`, `BlockFailed(file, line, exit_code)`, `BlockTimeout(file, line)`, `RepoUnreadable`.
- **Determinism:** D1. The checked commands are deterministic on one platform; shell and Python behaviour may differ across platforms, so CI runs it on each PLAT tier-1 OS.
- **Performance budget:** whole guide checked in under 60 s on a PLAT reference machine [unverified until measured].
- **Crate:** `fid-outreach`, module `doc_run`. Guide text lives in the docs portal source under `contributor/`.

## Test fixtures

This spec repository itself at the documented revision (CC-BY-4.0 text, tools under the code licence of D-001). Synthetic fixtures `fixtures/outr/contrib/broken-cmd` and `fixtures/outr/contrib/wrong-rev`, hand-written.

## Build prompt

> Write two things.
>
> (1) A contributor guide page (Markdown) for the docs portal of an open-source specification tree. Sections, in order: Find (list open nodes with `python tools/scope.py open`), Claim (check `claimed_by:` and open pull requests for the node ID before writing; set `claimed_by:`), Expand (follow `prompts/expand-node.md`: leaf, split or decision), Validate (`python tools/scope.py check`; fix errors in your files only), House rules (summarise `AGENTS.md` in your own words and link it), Building from source (link the implementation repository's build guide; until that exists, say so plainly). Front matter must state `revision: <git sha>`. Every command the reader should run goes in a fenced block with info string `sh run`; documented failing examples use `sh run expect-fail`. No marketing claims; no "certified" or "compliant" wording.
>
> (2) The `doc-run` subcommand of the Rust crate `fid-outreach`: `fid-outreach doc-run --guide <file> --repo <dir> [--json]`. It reads the guide, checks that its `revision:` equals `git rev-parse HEAD` of `--repo` (else exit 1 with `revision_mismatch`, running nothing), exports that revision to a temporary directory, runs each `sh run` block's lines in order there with a 60 s timeout per block, requires exit 0 (or non-zero for `expect-fail`), deletes the temporary directory, and exits 0 if all pass, 1 on any failure (naming guide file and block line), 2 on unreadable input. It must never modify the starting repository.
>
> Acceptance tests: the real guide passes on the current revision; `fixtures/outr/contrib/broken-cmd` exits 1 naming the line; `git status --porcelain` is unchanged after a run; `fixtures/outr/contrib/wrong-rev` exits 1 with `revision_mismatch`; the guide has the Find, Claim, Expand and Validate sections.
>
> Constraints: no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded reads; always clean up the temporary directory, also on failure; dependencies only under licences on the project allowlist; no network access.

## Open questions

- If ECO.COMM later replaces `tools/scope.py` with another tool, the guide and its runnable blocks change with it; the checker catches the drift.

## Children

_Leaf: none._
