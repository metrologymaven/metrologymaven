---
id: ECO.COMM.SCOPE
title: Spec-tree validator, open-node listing and generated reports
level: 3
parent: ECO.COMM
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.COMM-001, REQ-ECO.COMM-002, REQ-ECO.COMM-008]
covers: []
depends_on: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.COMM.SCOPE: Spec-tree validator, open-node listing and generated reports

## Purpose

`tools/scope.py`: the standard-library-only tool that validates the spec tree (frontmatter, IDs, levels, sections, traceability, seed coverage, dependency cycles, phase order), lists open work for people and agents, and regenerates `generated/INDEX.md`, `COVERAGE.md` and `DECISIONS.md`. A first version exists; this leaf states the contract it must meet, including two gaps in the current tool (claimed nodes are still listed as open work, and the tool is not yet tested on all three operating systems).

## Scope

**In:**
- `check`, `open` and `stats` subcommands and their exit codes
- All validation rules listed under Design
- Generated reports, marked "do not edit"
- Running on Windows, Linux and macOS with only the Python standard library
- Keeping `templates/*.md` and `prompts/expand-node.md` consistent with the rules this tool enforces (they are its inputs)

**Out:**
- The tree-growth driver (owned by `ECO.COMM.GROW`)
- Claim staleness and code-PR traceability (owned by `ECO.COMM.CLAIM`)
- Parity against the live registry (owned by `ECO.PARITY`)
- Running the check on PRs (owned by `ECO.CI.GATES` and `ECO.CI.WF`)

## Requirements

- **REQ-ECO.COMM.SCOPE-001** If any parent requirement is satisfied by no child, any seed covered by a parent is covered by no child, or any in-scope seed is covered by no node, then `check` shall report an error and exit 1.
- **REQ-ECO.COMM.SCOPE-002** `check` shall report as errors: missing required frontmatter fields or sections, bad or duplicate IDs, reserved Windows device names as segments, level and folder mismatches, leaves that are not size S or M or lack leaf sections, side-effect class or determinism class, feature leaves without CLI, MCP and journal names, decision status without a decision file, and dependency cycles.
- **REQ-ECO.COMM.SCOPE-003** `check` shall report as warnings, never errors: unknown `depends_on` targets, covers patterns that match no seed, a child whose phase is earlier than its parent's, and non-leaf nodes with size S or M.
- **REQ-ECO.COMM.SCOPE-004** `open` shall list only nodes with status `open` and an empty `claimed_by`, at the requested level (default: the shallowest level that has any), sorted by ID, one repo-relative path per line.
- **REQ-ECO.COMM.SCOPE-005** The tool shall import only Python standard-library modules and shall produce byte-identical reports on Windows, Linux and macOS for the same tree.
- **REQ-ECO.COMM.SCOPE-006** `check` on a tree of 2 000 nodes shall finish in under 10 s on the reference CI machine [unverified].

## Acceptance criteria

- **AC-1** Given fixture tree `fixtures/scope/valid/` (a root, two domains, four leaves, one seed file, one decision), when `python tools/scope.py check` runs with `ROOT` pointed at it, then it exits 0 and prints `0 errors`.
- **AC-2** (error path) Given `fixtures/scope/unsatisfied/` (a split parent whose REQ-X-002 no child lists), when `check` runs, then it exits 1 and prints `requirement REQ-X-002 is not satisfied by any child`.
- **AC-3** (error path) Given `fixtures/scope/cycle/` (A depends on B, B depends on A), when `check` runs, then it exits 1 with a `cycle:` error naming both.
- **AC-4** (error path) Given `fixtures/scope/reserved/` (a node with segment `CON`), when `check` runs, then it exits 1 with the reserved-name error.
- **AC-5** Given `fixtures/scope/claimed/` (three open level-2 nodes, one with `claimed_by: alice`), when `python tools/scope.py open --level 2` runs, then it prints the two unclaimed paths in ID order and not the claimed one.
- **AC-6** (warning) Given `fixtures/scope/phase/` (a P0 child under a P1a parent), when `check` runs, then it exits 0 and prints one `warn` line about phase order.
- **AC-7** (portability) Given `fixtures/scope/valid/`, when `check` runs on Windows, Linux and macOS in CI, then the three `generated/*.md` sets are byte-identical.
- **AC-8** (stdlib) A test imports `tools/scope.py` in an interpreter started with `-I -S` (no site packages) and it loads without error.

## Design

- **Registry command(s):** none; repository tool `python tools/scope.py check|open [--level N] [--limit N]|stats`.
- **Data structures:** `NODE.md` frontmatter (single-line lists), requirement lines `- **REQ-<id>-NNN**`, seed tables in `seeds/*.md`, out-of-scope table, decision frontmatter.
- **Algorithm:** load and parse all nodes; validate per node; validate parent-child traceability; phase order; Tarjan SCC for cycles over `depends_on` plus parent-to-child edges; global seed coverage; write reports with LF line endings and sorted content. Fixture trees are selected by a `--root` option (add it; default is the repository root).
- **Errors:** findings printed as `ERROR <file>: <msg>` or `warn  <file>: <msg>`; exit 1 if any error.
- **Determinism:** D0; sorted traversal, explicit UTF-8 and LF on write.
- **Performance budget:** REQ-006.
- **Crate:** not Rust; `tools/scope.py` in the spec repository (Python 3.9+, standard library only).

## Test fixtures

Small fixture trees under `tools/tests/fixtures/scope/` written for this leaf (CC-BY-4.0 like the spec text, or MIT OR Apache-2.0 as tooling).

## Build prompt

> In the spec repository, bring `tools/scope.py` (Python 3.9+, standard library only) to this contract and add tests under `tools/tests/` using `unittest`. Add a `--root <dir>` option so tests run against fixture trees. Change `open` to skip nodes whose `claimed_by` is non-empty. Keep every existing rule: errors for missing fields/sections, bad or duplicate IDs, reserved Windows names (CON, PRN, AUX, NUL, COM1-9, LPT1-9), level/folder mismatch, leaf size/kind/sections/side_effect/determinism, feature leaves needing cli/mcp/journal, decision status needing a decision file, unsatisfied parent requirements, dropped seeds, uncovered seeds, dependency cycles; warnings for unknown depends_on, unmatched covers patterns, child phase earlier than parent, S/M on non-leaves. Write reports with `newline="\n"` so output is byte-identical across OSes. Add fixtures `valid`, `unsatisfied`, `cycle`, `reserved`, `claimed`, `phase` and tests for AC-1..AC-8 of spec node ECO.COMM.SCOPE, and a CI job that runs the tests on Windows, Linux and macOS. Never import third-party modules.

## Open questions

- None blocking. The tool is in a separate PR from spec expansions (AGENTS.md working rule 6).

## Children

_Leaf: none._
