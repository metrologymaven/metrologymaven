---
id: ECO.RULES.SRC
title: Canonical house-rules source and its manifest
level: 3
parent: ECO.RULES
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [house-rules/]
built_pr: 85
satisfies: [REQ-ECO.RULES-001, REQ-ECO.RULES-006]
covers: []
depends_on: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.RULES.SRC: Canonical house-rules source and its manifest

## Purpose

Creates the one canonical place for contributor and agent rules: a `house-rules/` directory (in this spec repository until a `fid-contrib` repository exists) with a core block, language blocks, the restricted-path list, and a manifest that says which blocks go into which repository and what the size budgets are. Every other copy of the rules is generated from here by ECO.RULES.SYNC.

## Scope

**In:**
- `house-rules/manifest.toml`: block IDs, source files, target repositories, budgets
- `house-rules/core.md` (rules for every repository), `house-rules/rust.md`, `house-rules/python.md`
- `house-rules/restricted-paths.txt`: paths whose content must never be sent to external services (consumed by ECO.CI.BOTS)
- Content headings adopted from the Craft family (accountability, check then claim, report only what ran, ask only for high-stakes actions) plus this project's never-crash, clean-room and standards-text rules

**Out:**
- Block markers and digest (owned by `ECO.RULES.BLOCK`)
- Writing blocks into repositories (owned by `ECO.RULES.SYNC`)
- Enforcing the budgets (owned by `ECO.RULES.CHECK`)
- The SPDX allowlist (owned by `ECO.RULES.SPDX`) and the UI vocabulary rules (owned by `ECO.RULES.VOCAB`), which live in the same directory
- Governance policy (GOVERNANCE.md, D-003)

## Requirements

- **REQ-ECO.RULES.SRC-001** The manifest shall list every block with an ID, a source file and the repositories that must carry it, and every repository in the repository map shall receive the `core` block.
- **REQ-ECO.RULES.SRC-002** The manifest shall state the budgets: 3 072 bytes for the `core` block body and 10 240 bytes for a repository's whole `AGENTS.md`.
- **REQ-ECO.RULES.SRC-003** `core.md` shall contain headings for: accountability, check then claim, verify before citing, report only what ran, ask only for high-stakes actions, `main` is PR-only, never crash, clean room, and never paste standards text, and its body shall fit the 3 072-byte budget.
- **REQ-ECO.RULES.SRC-004** No file in `house-rules/` other than the generated block bodies shall be copied by hand into another repository; the README of the directory shall say so.

## Acceptance criteria

- **AC-1** Given the created `house-rules/` directory, when the test `tools/tests/test_house_rules_src.py` runs, then `manifest.toml` parses with Python's `tomllib` (or the JSON fallback `manifest.json` on Python < 3.11), lists blocks `core`, `rust`, `python`, and assigns `core` to every repository in `repos.toml` [the repository map from ECO.REPO.TOPO; until it exists, the test uses `house-rules/repos.fixture.toml`].
- **AC-2** `core.md` is at most 3 072 bytes in UTF-8 and contains each heading named in REQ-003.
- **AC-3** `restricted-paths.txt` exists and contains at least the patterns for standards excerpts and customer data used by the project's directory conventions.
- **AC-4** (error path) Given fixture `fixtures/house-rules/oversize-core.md` (3 073 bytes), when the same size test runs against it, then it fails with the measured size.

## Design

- **Registry command(s):** none; content plus a test.
- **Data structures:** `manifest.toml`: `[budgets] core_bytes = 3072, agents_md_bytes = 10240`; `[[block]] id, file, repos = [...]`.
- **Algorithm:** n/a (content). The test measures bytes after LF normalisation.
- **Errors:** test failures only.
- **Determinism:** D0.
- **Performance budget:** n/a.
- **Crate:** not Rust; files under `house-rules/` and one `unittest` test in `tools/tests/` (spec repository).

## Test fixtures

`fixtures/house-rules/oversize-core.md` (generated filler text, no third-party content).

## Build prompt

> In the spec repository, create `house-rules/` with `README.md` (states that this is the canonical source and copies are generated), `manifest.toml` (`[budgets] core_bytes = 3072, agents_md_bytes = 10240`; `[[block]]` entries `core` → `core.md` for all repositories, `rust` → `rust.md` for code repositories, `python` → `python.md` for the spec repository), `core.md` (≤ 3 072 bytes, headings: accountability; check then claim; verify before citing; report only what ran; ask only for high-stakes actions; main is PR-only; never crash; clean room; never paste standards text — condensed from AGENTS.md, in your own words), `rust.md` (denied lints, no unsafe, bounded parsers, crash-fix regression tests), `python.md` (standard library only for spec tooling), and `restricted-paths.txt`. Add `tools/tests/test_house_rules_src.py` (stdlib `unittest`) covering AC-1..AC-4 of spec node ECO.RULES.SRC. Do not copy text from other projects' rule files; paraphrase.

## Open questions

- When `fid-contrib` is created, this directory moves there and the spec repository becomes a sync target like any other.

## Children

_Leaf: none._
