---
id: PLAT.MATRIX.POLICY.ROWS
title: Matrix rows per D-010 and the promotion check
level: 4
parent: PLAT.MATRIX.POLICY
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-platform]
satisfies: [REQ-PLAT.MATRIX.POLICY-002, REQ-PLAT.MATRIX.POLICY-003, REQ-PLAT.MATRIX.POLICY-007]
covers: []
depends_on: [PLAT.MATRIX.FILE]
uses_artifacts: [PLAT.MATRIX.CI]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# PLAT.MATRIX.POLICY.ROWS: Matrix rows and promotion

## Purpose

Writes the rows of `platform/support-matrix.toml` as D-010 decided, and adds two checks: a window check that the Tier 1 rows are exactly the decided set (Windows 10 and 11 x86-64, the latest two Ubuntu LTS on x86-64, the latest two macOS majors on ARM64), and a promotion check that refuses a new Tier 1 row until its jobs passed on two consecutive release candidates.

## Scope

**In:**
- Rows (all from phase P1a unless noted): Windows 10 and Windows 11 x86-64 Tier 1; Ubuntu 22.04 and 24.04 LTS x86-64 Tier 1 [unverified: the latest two LTS at P1a release]; macOS latest two majors aarch64 Tier 1 [unverified: versions at P1a release]; Ubuntu LTS aarch64 Tier 2; Windows 11 aarch64 Tier 3; other listed distributions Tier 3
- `platform/tier1-window.toml`: the window rule (`ubuntu_lts = 2`, `macos_major = 2`, `windows = [10, 11]`)
- `xtask plat-matrix window-check` and `xtask plat-matrix promote-check`

**Out** (and which node owns it instead):
- Schema and lookup (`PLAT.MATRIX.FILE`)
- EOL demotion (`PLAT.MATRIX.POLICY.EOL`)
- Gate verdicts per release candidate (`PLAT.MATRIX.CI`)

## Requirements

- **REQ-PLAT.MATRIX.POLICY.ROWS-001** At phase P1a, the effective Tier 1 rows shall be exactly Windows 10 and Windows 11 on x86-64, the two newest Ubuntu LTS releases listed on x86-64 and the two newest macOS major releases listed on aarch64, and the window check shall fail naming any extra or missing Tier 1 row.
- **REQ-PLAT.MATRIX.POLICY.ROWS-002** The file shall list Ubuntu LTS on aarch64 at Tier 2 and Windows 11 on aarch64 at Tier 3 from P1a, per D-010.
- **REQ-PLAT.MATRIX.POLICY.ROWS-003** The window check shall exclude from the expected Tier 1 set any row demoted under the EOL rule (its notes carry a `demoted:` entry).
- **REQ-PLAT.MATRIX.POLICY.ROWS-004** When a row's tier is raised to Tier 1 in a change, the promotion check shall pass only if the gate verdict files of the two most recent release candidates both show that row's Tier 1 jobs passed, and shall fail naming the row otherwise.
- **REQ-PLAT.MATRIX.POLICY.ROWS-005** The window check and promotion check shall produce byte-identical output for the same inputs.

## Acceptance criteria

- **AC-1** Given the shipped matrix and window file at P1a, when `cargo xtask plat-matrix window-check --phase P1a` runs, then it exits 0.
- **AC-2** (error path) Given `fixtures/plat/matrix/rows-three-ubuntu.toml` with three Tier 1 Ubuntu LTS rows, then `OutsideWindow { os: ubuntu, version }` for the oldest.
- **AC-3** (error path) Given a matrix with Windows 11 aarch64 at Tier 1, then `OutsideWindow`.
- **AC-4** Given `fixtures/plat/matrix/promote-old.toml` and `promote-new.toml` (Linux aarch64 raised to Tier 1) and gate verdicts `rc1-pass.json`, `rc2-pass.json` (its jobs passed), when `promote-check` runs, then it exits 0; with `rc2-fail.json`, it exits 1 with `PromotionNotEarned`.
- **AC-5** Given a row with `demoted:` notes, then it is not expected in the window.

## Design

- **Registry command(s):** none; data and `xtask` checks.
- **Data structures:** `Tier1Window { ubuntu_lts: u8, macos_major: u8, windows: Vec<u32> }`; promotion compares two matrix files (`--old`, `--new`) and reads gate verdict JSON from `PLAT.MATRIX.CI`.
- **Algorithm:** window: collect listed Ubuntu LTS (even year, `.04`) and macOS majors from rows, take the newest N not demoted, compare with effective Tier 1 rows. Promotion: diff tiers between old and new; for each raised row, look up its job IDs in the two latest verdict files.
- **Errors:** `OutsideWindow`, `MissingFromWindow`, `PromotionNotEarned`, `NoVerdicts`.
- **Determinism:** D0.
- **Crate:** `fid-platform` (module `matrix_policy`) plus `xtask`.

## Test fixtures

Matrix and verdict fixtures under `fixtures/plat/matrix/`. Licensed with the code.

## Build prompt

> Update `platform/support-matrix.toml` rows per D-010 (all since P1a): Windows 10 and 11 x86-64 tier1; the two newest Ubuntu LTS x86-64 tier1; the two newest macOS majors aarch64 tier1; Ubuntu LTS aarch64 tier2; Windows 11 aarch64 tier3; mark version choices [unverified] in notes. Add `platform/tier1-window.toml` (ubuntu_lts = 2, macos_major = 2, windows = [10, 11]). In crate `fid-platform` add module `matrix_policy` and `xtask plat-matrix window-check --phase <P>` (effective Tier 1 rows must equal the window, excluding rows with `demoted:` notes) and `xtask plat-matrix promote-check --old <matrix> --new <matrix> --verdicts <dir>` (a row raised to tier1 needs its Tier 1 jobs passed in the two latest release-candidate gate verdicts from PLAT.MATRIX.CI). Byte-stable output. Tests: AC-1..AC-5. Constraints: CONSTITUTION §8; `BTreeMap` only. Commit: `feat(fid-platform): matrix rows and promotion check`.

## Open questions

- The exact Ubuntu and macOS versions are set when P1a ships; the window rule, not the version numbers, is binding.

## Children

_Leaf: none._
