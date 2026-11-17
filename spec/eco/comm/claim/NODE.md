---
id: ECO.COMM.CLAIM
title: Claim staleness and code-to-node traceability
level: 3
parent: ECO.COMM
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.COMM-003, REQ-ECO.COMM-004]
covers: []
depends_on: [ECO.COMM.SCOPE]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.COMM.CLAIM: Claim staleness and code-to-node traceability

## Purpose

Two small checks that keep "check, then claim" honest. The stale-claim report finds nodes whose `claimed_by:` is older than 14 days with no linked PR, so someone else may take them over. The traceability check refuses a code PR that does not name a spec leaf, so every unit of implementation work traces to a node (REQ-ECO-009).

## Scope

**In:**
- `tools/claims.py stale`: compute stale claims from the spec tree, git history and an exported list of open PRs
- `tools/claims.py trace`: validate a code PR's title and body against the spec tree at a pinned revision
- The `Implements: <NODE.ID>` convention for code PRs
- Issue and PR templates, which carry the `Implements:` line and the claim checklist

**Out:**
- Posting labels or comments on the forge (a thin CI workflow in `ECO.CI.WF` acts on this tool's JSON output)
- The validator (owned by `ECO.COMM.SCOPE`)
- PR timelines (owned by `ECO.COMM.TIMELINE`)

## Requirements

- **REQ-ECO.COMM.CLAIM-001** The claim date of a node shall be the commit date of the commit that last changed its `claimed_by:` line to a non-empty value, taken from repository history.
- **REQ-ECO.COMM.CLAIM-002** When a node's claim date is more than 14 days before `--now` and no open PR in the supplied PR list names the node ID in its title, body or branch name, the stale report shall list the node with its claimant and claim date.
- **REQ-ECO.COMM.CLAIM-003** If a code PR's title or body contains no `Implements: <NODE.ID>` line, then the trace check shall exit 1 with `TRACE-NONE`.
- **REQ-ECO.COMM.CLAIM-004** If a named node does not exist in the spec tree at the pinned revision, or exists but its status is not `leaf` or `frozen`, then the trace check shall exit 1 with `TRACE-UNKNOWN` or `TRACE-NOT-LEAF`.
- **REQ-ECO.COMM.CLAIM-005** PRs labelled `no-spec` by a maintainer (documentation typos, dependency bumps, CI-only changes) shall pass the trace check, and the label shall be listed in the tool's output.
- **REQ-ECO.COMM.CLAIM-006** Both subcommands shall use only the Python standard library and `git`, read the PR data from a JSON file, and produce sorted, byte-stable JSON output.

## Acceptance criteria

- **AC-1** Given fixture repository `fixtures/claims/repo.bundle` (node A claimed 20 days before `--now`, node B claimed 3 days before, node C claimed 20 days before with an open PR titled "spec(C): leaf") and `fixtures/claims/prs.json`, when `python tools/claims.py stale --repo <clone> --prs fixtures/claims/prs.json --now 2026-12-01T00:00:00Z --json` runs, then the output lists exactly node A.
- **AC-2** Given `fixtures/claims/pr-ok.json` (body has `Implements: ALN.RPS.ITER`, a leaf in the fixture tree), when `claims.py trace --pr fixtures/claims/pr-ok.json --tree fixtures/claims/tree` runs, then it exits 0.
- **AC-3** (error path) Given `pr-none.json`, when trace runs, then it exits 1 with `TRACE-NONE`.
- **AC-4** (error path) Given `pr-open-node.json` naming a node with status `open`, when trace runs, then it exits 1 with `TRACE-NOT-LEAF <id>`.
- **AC-5** Given `pr-nospec.json` with label `no-spec` and no `Implements:` line, when trace runs, then it exits 0 and the JSON output contains `"exempt": "no-spec"`.
- **AC-6** (determinism) Running AC-1 twice yields byte-identical output.

## Design

- **Registry command(s):** none; repository tool `python tools/claims.py stale|trace`.
- **Data structures:** PR list JSON: `[{number, title, body, head_ref, labels[], state}]` (the shape `gh pr list --json` can produce [unverified field names]); output `{"stale":[{"id","claimed_by","claimed_at"}]}` or `{"pr":n,"nodes":[...],"result":"ok|fail","codes":[...],"exempt":null|"no-spec"}`.
- **Algorithm:** for each node with non-empty `claimed_by`, run `git log -1 --format=%cI -S"claimed_by: <value>" -- <file>`; compare against `--now`; match node IDs in PR text with a word-boundary regex. Trace: extract `^Implements:\s*([A-Z0-9.]+)` lines; load the tree with SCOPE's parser.
- **Errors:** `TRACE-NONE`, `TRACE-UNKNOWN`, `TRACE-NOT-LEAF`, `CLAIMS-GIT` (git unavailable), `CLAIMS-INPUT` (bad JSON).
- **Determinism:** D0 given `--now`; the tool never reads the system clock when `--now` is supplied, and CI always supplies it.
- **Performance budget:** under 10 s for 2 000 nodes and 500 PRs [unverified].
- **Crate:** not Rust; `tools/claims.py` in the spec repository, standard library only, reusing `scope.py`'s node parser.

## Test fixtures

A git bundle built by a fixture script (`tools/tests/fixtures/claims/make_repo.py`) with fixed author dates, plus hand-written PR JSON files.

## Build prompt

> In the spec repository, write `tools/claims.py` (Python 3.9+, stdlib only, may import `scope.py`'s parser). Subcommand `stale --repo <dir> --prs <json> --now <ISO8601> [--json]`: for each node with non-empty `claimed_by`, find the date of the latest commit that set that value (`git log -1 --format=%cI -S"claimed_by: <v>" -- <path>`); list nodes whose claim is older than 14 days and whose ID appears (word boundary) in no open PR's title, body or head branch. Subcommand `trace --pr <json> --tree <dir>`: extract `Implements: <ID>` lines; fail `TRACE-NONE` if none (unless label `no-spec`), `TRACE-UNKNOWN` if the ID is not in the tree, `TRACE-NOT-LEAF` if its status is not leaf or frozen. Output sorted JSON; exit 0/1. Build a fixture repo with a script that sets `GIT_AUTHOR_DATE`/`GIT_COMMITTER_DATE`, and write `unittest` tests for AC-1..AC-6 of spec node ECO.COMM.CLAIM. Never call the forge API from this tool; CI passes exported JSON.

## Open questions

- Takeover etiquette (comment first, then reassign) belongs in CONTRIBUTING.md; the tool only reports.

## Children

_Leaf: none._
