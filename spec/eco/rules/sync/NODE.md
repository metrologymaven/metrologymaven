---
id: ECO.RULES.SYNC
title: House-rules sync tool
level: 3
parent: ECO.RULES
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.RULES-007, REQ-ECO.RULES-008]
covers: []
depends_on: [ECO.RULES.SRC, ECO.RULES.BLOCK]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
built_in: [tools/house_rules]
built_pr: 464
updated: 2026-11-22
---

# ECO.RULES.SYNC: House-rules sync tool

## Purpose

Writes or refreshes the generated house-rules blocks in a target repository's `AGENTS.md`, and writes its `CLAUDE.md` as a single reference to `AGENTS.md`, from the canonical source at a given revision. It touches nothing outside the markers, and running it twice changes nothing the second time.

## Scope

**In:**
- `python -m house_rules sync --source <dir> --source-rev <rev> --repo-id <id> --target <dir> [--dry-run]`
- Inserting missing blocks (at the top of `AGENTS.md`, after the first heading), replacing existing ones, removing blocks no longer assigned
- Writing `CLAUDE.md`

**Out:**
- Block format (owned by `ECO.RULES.BLOCK`)
- Pass/fail checks (owned by `ECO.RULES.CHECK`)
- Opening PRs in target repositories (a maintainer or a scheduled workflow does that with the output)

## Requirements

- **REQ-ECO.RULES.SYNC-001** When sync runs, every block assigned to the target repository in the manifest shall be present in its `AGENTS.md` with the body of the source file at `--source-rev` and the matching digest.
- **REQ-ECO.RULES.SYNC-002** Sync shall not change any byte of `AGENTS.md` outside generated blocks, except to insert a missing block after the first heading line.
- **REQ-ECO.RULES.SYNC-003** When sync runs twice with the same source revision, the second run shall change no file and shall report "no changes".
- **REQ-ECO.RULES.SYNC-004** When sync runs, the target's `CLAUDE.md` shall contain exactly the line `@AGENTS.md` followed by a newline.
- **REQ-ECO.RULES.SYNC-005** If a block in the target was edited by hand (digest mismatch), then sync shall replace it with the canonical body and list it as "restored".
- **REQ-ECO.RULES.SYNC-006** With `--dry-run`, sync shall print the changes it would make and write nothing.

## Acceptance criteria

- **AC-1** Given source `fixtures/sync/source/` and target `fixtures/sync/target-empty/` (an `AGENTS.md` with only a heading and a paragraph, no `CLAUDE.md`), when sync runs, then `AGENTS.md` contains the `core` and `rust` blocks after the heading, the paragraph is unchanged, and `CLAUDE.md` is `@AGENTS.md\n`.
- **AC-2** (idempotence) Running AC-1's command again changes no file (byte comparison) and prints `no changes`.
- **AC-3** Given `target-edited/` (the core block body edited by hand), when sync runs, then the body is restored and the output lists `restored core`.
- **AC-4** Given `target-extra/` (a block `python` not assigned to this repository), when sync runs, then that block is removed and nothing else changes.
- **AC-5** Given `--dry-run` on `target-empty/`, then no file changes and the planned changes are printed.
- **AC-6** Given `target-crlf/` (an `AGENTS.md` with CRLF line endings), when sync runs, then text outside blocks keeps CRLF endings.

## Design

- **Registry command(s):** none; repository tool. `side_effect: W1`: writes only inside the target working tree.
- **Data structures:** manifest from ECO.RULES.SRC; `Block` from ECO.RULES.BLOCK.
- **Algorithm:** read manifest at the source revision (the caller checks out that revision, or passes `--source` pointing at it); compute desired blocks; parse target; build new text by splicing; write via temp file and rename only if bytes differ.
- **Errors:** `SYNC-SOURCE` (missing block file), `SYNC-TARGET` (unparseable blocks), `SYNC-REPO` (repo ID not in manifest).
- **Determinism:** D0.
- **Performance budget:** under 1 s per repository.
- **Crate:** not Rust; Python package `tools/house_rules/` (module `sync.py`), standard library only.

## Test fixtures

Small source and target trees under `tools/tests/fixtures/sync/`.

## Build prompt

> In `tools/house_rules/` (Python stdlib, uses `block.py`), add `sync.py` and a `__main__` entry: `python -m house_rules sync --source <dir> --source-rev <40-hex> --repo-id <id> --target <dir> [--dry-run]`. Read the manifest (`manifest.toml`, or `manifest.json` on Python < 3.11); for the repo ID, render each assigned block from its source file; replace existing blocks, insert missing ones after the first Markdown heading line, remove blocks not assigned; restore hand-edited blocks and report `restored <id>`; write `CLAUDE.md` as `@AGENTS.md\n`; preserve original line endings outside blocks; write only if bytes change (temp file + rename); print `no changes` when nothing changed. Errors `SYNC-SOURCE`, `SYNC-TARGET`, `SYNC-REPO`. Write `unittest` tests for AC-1..AC-6 of spec node ECO.RULES.SYNC.

## Open questions

- A scheduled workflow that runs sync across all repositories and opens PRs belongs to ECO.CI.WF once there is more than one repository.

## Children

_Leaf: none._
