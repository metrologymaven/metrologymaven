---
id: ECO.REPO.TPL.FILES
title: Template file set and instantiation script
level: 4
parent: ECO.REPO.TPL
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.REPO.TPL-002]
covers: []
depends_on: []
uses_artifacts: [ECO.RULES.BLOCK, ECO.RULES.SYNC, ECO.CI.WF, ECO.REPO.LAYERS, ECO.REPO.TOPO]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.REPO.TPL.FILES: Template file set and instantiation script

## Purpose

The template directory with two variants (`code` for a Rust workspace, `spec` for a documentation or spec repository) and a standard-library Python script that copies a variant into a new folder, fills in the repository ID and pinned revisions, writes the current house-rules block by calling the sync tool, and refuses to leave any placeholder unfilled.

## Scope

**In:**
- `templates/repo/common/`: `LICENSE-MIT`, `LICENSE-APACHE`, `ATTRIBUTION.md`, `AGENTS.md` skeleton with a block slot, `CLAUDE.md` (`@AGENTS.md` only), `CONTRIBUTING.md` pointer, `.gitattributes` (LF), CI caller workflow pinned to a revision
- `templates/repo/code/`: workspace `Cargo.toml` with inherited never-crash lints, `layers.toml` with one starter crate, `rust-toolchain.toml`, a starter crate that passes the lints
- `templates/repo/spec/`: minimal `spec/NODE.md` skeleton and tool configuration
- `python -m repo_template new --id <repo-id> --variant code|spec --rules-source <dir> --rules-rev <40-hex> --ci-rev <40-hex> --out <dir>`
- `variants.toml` listing variants and their files

**Out:**
- Running the shared checks on the result (owned by `ECO.REPO.TPL.SELFTEST`)
- Block format and sync logic (owned by `ECO.RULES.BLOCK` and `ECO.RULES.SYNC`)
- Adding the new repository to `repos.toml` in the hub repository (a PR by the maintainer, checked by `ECO.REPO.TOPO`)

## Requirements

- **REQ-ECO.REPO.TPL.FILES-001** Every instantiated repository shall contain `CLAUDE.md` whose only content is the line `@AGENTS.md` followed by one newline.
- **REQ-ECO.REPO.TPL.FILES-002** The instantiation script shall write the house-rules blocks assigned to the new repository ID by running the house-rules sync tool at the given source revision, and shall fail if the sync tool fails.
- **REQ-ECO.REPO.TPL.FILES-003** If any `{{placeholder}}` token remains in any output file after instantiation, then the script shall fail, delete the partial output and name the file and token.
- **REQ-ECO.REPO.TPL.FILES-004** The CI caller workflow in the output shall reference the shared workflows by the 40-hex revision given on the command line, never a branch or tag.
- **REQ-ECO.REPO.TPL.FILES-005** The `code` variant's `layers.toml` shall assign its starter crate to a layer, and its workspace lints shall deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable` and `unsafe_code`.
- **REQ-ECO.REPO.TPL.FILES-006** If the output directory exists and is not empty, then the script shall refuse to write and exit non-zero.

## Acceptance criteria

- **AC-1** Given fixture rules source `fixtures/repo/tpl/rules/` and revisions `aaaa…` (40 hex) for rules and CI, when `python -m repo_template new --id fid-demo --variant code --rules-source fixtures/repo/tpl/rules --rules-rev <a> --ci-rev <b> --out out/` runs, then it exits 0, `out/CLAUDE.md` equals `@AGENTS.md\n`, `out/AGENTS.md` contains a valid `core` block with source revision `<a>`, and the caller workflow references `@<b>`.
- **AC-2** (error path) Given a template file with an unknown placeholder `{{owner}}`, then it exits 1 naming the file and token and `out/` does not exist.
- **AC-3** (error path) Given `--out` pointing at a non-empty directory, then it exits 1 and changes nothing.
- **AC-4** Given the `code` variant output, when `cargo clippy --workspace -- -D warnings` runs, then it passes, and the starter crate is listed in `layers.toml`.
- **AC-5** Given the `spec` variant, when instantiated, then the output has no `Cargo.toml` and the same common files.

## Design

- **Registry command(s):** none; Python standard-library package `tools/repo_template/` (`python -m repo_template new`). Writes only into the new output folder (W1).
- **Data structures:** `variants.toml` `[[variant]] id, include = [common, code]`; placeholders `{{repo_id}}`, `{{ci_rev}}`, `{{year}}` (from the rules revision's commit date, not the clock).
- **Algorithm:** copy into a temporary sibling folder, substitute placeholders, run `python -m house_rules sync` for the repository ID, scan for leftovers, then rename into place atomically.
- **Errors:** `TPL-PLACEHOLDER(file, token)`, `TPL-OUT-NOT-EMPTY`, `TPL-SYNC-FAILED`.
- **Determinism:** D0 given the same inputs.
- **Performance budget:** under 5 s.
- **Crate:** not Rust; Python package `tools/repo_template/` (standard library only), templates under `templates/repo/`.

## Test fixtures

`fixtures/repo/tpl/` with a small rules source, a bad-placeholder variant and a non-empty output folder; CC-BY-4.0.

## Build prompt

> In the hub repository create `templates/repo/{common,code,spec}/` and `templates/repo/variants.toml`, plus Python standard-library package `tools/repo_template/` with `python -m repo_template new --id <repo-id> --variant code|spec --rules-source <dir> --rules-rev <40-hex> --ci-rev <40-hex> --out <dir>`. Common files: LICENSE-MIT, LICENSE-APACHE, ATTRIBUTION.md, AGENTS.md skeleton, CLAUDE.md containing exactly `@AGENTS.md\n`, CONTRIBUTING.md pointer, .gitattributes forcing LF, a CI caller workflow referencing the shared workflows at `{{ci_rev}}`. Code variant: workspace Cargo.toml with workspace lints denying unwrap_used, expect_used, panic, todo, unimplemented, unreachable and unsafe_code; rust-toolchain.toml; layers.toml with the starter crate; a starter crate that passes clippy. Spec variant: minimal spec skeleton. The script copies into a temporary folder, substitutes placeholders, runs `python -m house_rules sync` for the repo ID at the given rules revision, fails on any leftover `{{...}}` token (TPL-PLACEHOLDER) or sync failure (TPL-SYNC-FAILED), refuses a non-empty output (TPL-OUT-NOT-EMPTY), then moves the result into place. Tests (unittest): AC-1..AC-5 of spec node ECO.REPO.TPL.FILES.

## Open questions

- The `{{year}}` in licence headers comes from the rules revision date to keep output deterministic; maintainers may prefer no year at all.

## Children

_Leaf: none._
