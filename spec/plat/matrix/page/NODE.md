---
id: PLAT.MATRIX.PAGE
title: Generated support matrix page and consistency check
level: 3
parent: PLAT.MATRIX
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-platform]
built_pr: 159
satisfies: [REQ-PLAT.MATRIX-002]
covers: []
depends_on: [PLAT.MATRIX.FILE]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# PLAT.MATRIX.PAGE: Generated support matrix page

## Purpose

Generates the human-readable support matrix page from the machine-readable file, and gives CI a `--check` mode that fails when the committed page and the file disagree. The page is never hand-edited (CONSTITUTION §8, honest status).

## Scope

**In:**
- Rendering `platform/support-matrix.toml` to Markdown at `docs/platform/support-matrix.md`
- A "generated, do not edit" header carrying the source file's SHA-256
- `--check` mode for CI

**Out** (and which node owns it instead):
- The matrix data and schema (`PLAT.MATRIX.FILE`, `PLAT.MATRIX.POLICY`)
- Publishing the docs site (`OUTR.DOCS`)
- Wiring the check into the required-check set (`ECO.CI`)

## Requirements

- **REQ-PLAT.MATRIX.PAGE-001** When run, the generator shall write one Markdown table per OS family with columns version range, architecture, tier, since phase and notes, sorted by (os, arch, version_min).
- **REQ-PLAT.MATRIX.PAGE-002** The generated page shall begin with a header stating it is generated and giving the SHA-256 of the source matrix file.
- **REQ-PLAT.MATRIX.PAGE-003** When run with `--check`, the generator shall exit 1 and print a unified diff if the committed page differs from what it would generate, and exit 0 otherwise, without writing.
- **REQ-PLAT.MATRIX.PAGE-004** The generator shall produce byte-identical output for the same matrix file on every platform, with LF line endings.

## Acceptance criteria

- **AC-1** Given `fixtures/plat/matrix/valid.toml`, when `cargo xtask plat-matrix page --out <tmp>/page.md` runs, then the output equals `fixtures/plat/matrix/expected-page.md` byte for byte.
- **AC-2** Given `expected-page.md` with one tier cell edited by hand, when `cargo xtask plat-matrix page --check --out <that file>` runs, then it exits 1 and the diff names the edited line; the file is unchanged.
- **AC-3** Given the unedited page, when `--check` runs, then it exits 0.
- **AC-4** (error path) Given `fixtures/plat/matrix/overlap.toml`, when the generator runs, then it exits 1 with the validator's `OverlappingRange` error and writes nothing.

## Design

- **Registry command(s):** none; `xtask plat-matrix page [--check] [--matrix <file>] --out <file>`.
- **Data structures:** none new; uses `fid-platform::matrix::Matrix`.
- **Algorithm:** validate the matrix (fail on any error); group rows by OS family in a fixed family order; render a Markdown table per group; escape `|` in notes; header line with SHA-256 of the raw source bytes; normalise to LF.
- **Errors:** matrix validation errors; `Io` on the output path; `Mismatch` in check mode.
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-platform` (module `page`) with the `xtask` subcommand.

## Test fixtures

`fixtures/plat/matrix/valid.toml`, `overlap.toml` (shared with PLAT.MATRIX.FILE) and `expected-page.md` (hand-checked once, then golden).

## Build prompt

> In crate `fid-platform`, add module `page` and the `xtask` subcommand `plat-matrix page [--check] [--matrix <file>] --out <file>`. It validates the support matrix with `fid-platform::matrix`, then renders Markdown: a header line `<!-- generated from platform/support-matrix.toml sha256:<hex>; do not edit -->`, then one `##` section per OS family (fixed order: windows, macos, ubuntu, debian, fedora, rhel) with a table `| Versions | Arch | Tier | Since | Notes |`, rows sorted by (arch, version_min), `|` in notes escaped, LF line endings. `--check` compares with the existing file, prints a unified diff and exits 1 on difference, never writes. Validation failure exits 1 and writes nothing.
>
> Acceptance tests: the four criteria above with fixtures in `fixtures/plat/matrix/`. Constraints: CONSTITUTION §8 (no panic, no `unwrap`, no `unsafe`); output byte-identical across OSes. Commit: `feat(fid-platform): generated support matrix page`.

## Open questions

- None.

## Children

_Leaf: none._
