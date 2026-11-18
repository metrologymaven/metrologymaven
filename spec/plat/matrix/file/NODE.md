---
id: PLAT.MATRIX.FILE
title: Support matrix schema, data file and validator
level: 3
parent: PLAT.MATRIX
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-plat-matrix]
built_pr: 76
satisfies: [REQ-PLAT.MATRIX-001, REQ-PLAT.MATRIX-007]
covers: []
depends_on: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# PLAT.MATRIX.FILE: Support matrix schema, data file and validator

## Purpose

Defines the machine-readable support matrix (`platform/support-matrix.toml`), its typed schema, a strict validator and a lookup function. Every other PLAT consumer (CI job list, docs page, `fid doctor`, GPU policy, SDK table) reads the matrix only through this crate, so there is exactly one parser and one meaning of "tier".

## Scope

**In:**
- The TOML schema: `schema_version`, and rows of `os`, `version_min`, `version_max`, `arch`, `tier`, `since_phase`, `notes`
- Parsing with bounded input and typed errors
- Validation: exactly one tier per combination, no overlapping version ranges, valid phase codes, known OS and architecture names
- `lookup(os, version, arch, as_of_phase)` returning the effective tier

**Out** (and which node owns it instead):
- Which rows and tiers the file contains, and tier policy (`PLAT.MATRIX.POLICY`)
- The CI job list and release gate (`PLAT.MATRIX.CI`)
- The generated docs page (`PLAT.MATRIX.PAGE`)
- Browser rows (`PLAT.WEB.BROWSERS` extends the schema through a reviewed schema bump)

## Requirements

- **REQ-PLAT.MATRIX.FILE-001** The crate shall define a versioned matrix schema whose rows carry `os`, `version_min`, `version_max`, `arch`, `tier`, `since_phase` and `notes`, with `tier` one of `tier1`, `tier2` or `tier3`.
- **REQ-PLAT.MATRIX.FILE-002** When a matrix file is validated, the validator shall reject the file if any two rows with the same `os` and `arch` have overlapping version ranges, naming both rows.
- **REQ-PLAT.MATRIX.FILE-003** If a row's `since_phase` is missing or is not a CONSTITUTION §7 phase code, then validation shall fail and name the row.
- **REQ-PLAT.MATRIX.FILE-004** When `lookup` is called with an OS, version, architecture and current phase, the crate shall return the matching row's tier if the current phase is at or after `since_phase`, `planned(tier, since_phase)` if it is before, and `not_listed` if no row matches.
- **REQ-PLAT.MATRIX.FILE-005** If the file exceeds 1 MiB or 10 000 rows, then parsing shall fail closed with a typed `TooLarge` error.
- **REQ-PLAT.MATRIX.FILE-006** If `schema_version` is greater than the highest version the reader supports, then parsing shall fail with a typed `UnsupportedSchema` error and no partial result.
- **REQ-PLAT.MATRIX.FILE-007** Validating the same file twice shall produce byte-identical diagnostics in the same order.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/plat/matrix/valid.toml` (6 rows across windows, ubuntu and macos), when `cargo xtask plat-matrix validate fixtures/plat/matrix/valid.toml` runs, then it exits 0 and prints `6 rows, 0 errors`.
- **AC-2** (error path) Given `fixtures/plat/matrix/overlap.toml`, where two `ubuntu`/`x86_64` rows cover `[22.04, 24.10)` and `[24.04, 26.04)`, when validated, then it exits 1 with one `OverlappingRange` error naming rows 2 and 3.
- **AC-3** (error path) Given `fixtures/plat/matrix/bad-phase.toml` with `since_phase = "P9"`, when validated, then it exits 1 with `BadPhase` naming the row.
- **AC-4** Given `valid.toml`, when `lookup("macos", "15.1", "aarch64", P0)` is called for a row with `since_phase = "P1a"` and `tier = "tier1"`, then the result is `planned(tier1, P1a)`; with `P1a` it is `tier1`; and `lookup("freebsd", "14", "x86_64", P1a)` is `not_listed`.
- **AC-5** (bounds) Given a generated 10 001-row file (generator seed 1 in the test), when parsed, then the result is `TooLarge` within 1 s and nothing panics.
- **AC-6** Given `fixtures/plat/matrix/future-schema.toml` with `schema_version = 99`, when parsed, then the result is `UnsupportedSchema { found: 99 }`.
- **AC-7** (determinism) Given `overlap.toml`, when validated twice, then the two diagnostic outputs are byte-identical.

## Design

- **Registry command(s):** none; this is an internal library and an `xtask` subcommand used by CI and docs. User-facing access is through `fid doctor` (`PLAT.DOCTOR.CMD`).
- **Data structures:** `Matrix { schema_version: u32, rows: Vec<Row> }`; `Row { os: OsFamily, version_min: Version, version_max: Option<Version>, arch: Arch, tier: Tier, since_phase: Phase, notes: String }`. `OsFamily` = `windows | ubuntu | debian | fedora | rhel | macos` (extensible by schema bump). `Arch` = `x86_64 | aarch64`. `Version` is a dotted numeric tuple (up to 4 components, each u32); Windows rows use the build number form (`10.0.19041`) so Windows 10 and 11 are distinct ranges. Ranges are half-open `[min, max)`; `max = None` means open-ended. `EffectiveTier = Tier(Tier) | Planned(Tier, Phase) | NotListed`.
- **Algorithm:** parse TOML with a size check before parse; validate each row; sort rows by (os, arch, version_min) with a stable sort, then check adjacent pairs for overlap; diagnostics collected in row order. Lookup is a linear scan (the file is small).
- **Errors:** `TooLarge`, `UnsupportedSchema`, `Parse { line, msg }`, `BadPhase { row }`, `UnknownOs { row }`, `UnknownArch { row }`, `BadVersion { row }`, `OverlappingRange { a, b }`, `EmptyRange { row }`.
- **Determinism:** D0; pure functions over bytes, no hash-ordered containers.
- **Performance budget:** validate a 1 000-row file in < 50 ms on the minimum profile [unverified until measured].
- **Crate:** `fid-platform` (module `matrix`), plus the `plat-matrix validate` subcommand in the workspace `xtask`.

## Test fixtures

Hand-written TOML files under `fixtures/plat/matrix/` (`valid.toml`, `overlap.toml`, `bad-phase.toml`, `future-schema.toml`), licensed with the code (MIT OR Apache-2.0, D-001). The oversized file is generated inside the test with a fixed seed.

## Build prompt

> In the implementation repository, create the library crate `fid-platform` (if absent) with a module `matrix`, and add an `xtask` subcommand `plat-matrix validate <file>`.
>
> Implement a strict parser and validator for the support-matrix TOML file `platform/support-matrix.toml`: top-level `schema_version` (u32; this reader supports 1) and an array `[[row]]` with fields `os` (windows, ubuntu, debian, fedora, rhel, macos), `version_min` and optional `version_max` (dotted numeric versions, up to 4 u32 components, half-open range), `arch` (x86_64, aarch64), `tier` (tier1, tier2, tier3), `since_phase` (P0, P1a, P1b, P2, P3, P4, later) and `notes` (string).
>
> Requirements: reject files over 1 MiB or 10 000 rows with `TooLarge` before full parse; reject `schema_version` above 1 with `UnsupportedSchema`; reject overlapping ranges for the same os and arch (name both rows), bad phases, unknown OS or arch, malformed or empty ranges. Provide `lookup(os, version, arch, current_phase) -> EffectiveTier` returning `Tier`, `Planned(tier, since_phase)` when the current phase is earlier than `since_phase`, or `NotListed`. Diagnostics are deterministic and in row order.
>
> Acceptance tests: the seven criteria above, using fixtures in `fixtures/plat/matrix/` (create them). Constraints: no panic, no `unwrap`/`expect`, no `unsafe` (CONSTITUTION §8); bounded parsing that fails closed; no std `HashMap`/`HashSet` in output paths, use `BTreeMap` or sorted `Vec`. Commit message: `feat(fid-platform): support matrix schema and validator`.

## Open questions

- Additional Linux distributions (Debian, Fedora, RHEL) are in the enum so rows can be listed as Tier 2 or 3; which ones are listed is `PLAT.MATRIX.POLICY`'s call.

## Children

_Leaf: none._
