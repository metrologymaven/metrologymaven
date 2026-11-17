---
id: AUTO.PY.VER
title: Supported CPython versions and test matrix
level: 3
parent: AUTO.PY
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-AUTO.PY-011]
covers: []
depends_on: [AUTO.PY.GEN]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.PY.VER: Supported CPython versions and test matrix

## Purpose

One machine-readable list of supported CPython versions that drives both the published support statement and the CI test matrix, so the docs cannot claim a version CI does not test.

## Scope

**In:**
- `python-versions.json` (supported versions with start and planned end dates)
- Policy text: support every CPython feature release that is not end-of-life at our release date [unverified against the CPython release schedule]
- CI matrix generation from the file; docs table generation from the file

**Out:**
- CI runner configuration (`ECO.CI`)
- Interpreter installation (`PLAT`)

## Requirements

- **REQ-AUTO.PY.VER-001** The supported versions shall be listed in exactly one file, `python-versions.json`.
- **REQ-AUTO.PY.VER-002** The CI test matrix and the published support table shall be generated from that file, and a check shall fail if either differs from it.
- **REQ-AUTO.PY.VER-003** If a listed version has passed its end-of-life date in the file, then the check shall fail until the version is removed or the date is updated.

## Acceptance criteria

- **AC-1** Given `python-versions.json` listing 3.11, 3.12 and 3.13, when `cargo xtask py-matrix` runs, then the generated CI matrix and `docs/python-support.md` list exactly those versions.
- **AC-2** (error path) Given a hand edit adding 3.9 to the generated CI matrix, then `cargo xtask py-matrix --check` fails naming the difference.
- **AC-3** (error path) Given a version with an end-of-life date before the check's reference date, then the check fails naming it.

## Design

- **Registry command(s):** none.
- **Data structures:** `[{ "version": "3.12", "supported_from": "YYYY-MM-DD", "eol": "YYYY-MM-DD" }]`.
- **Algorithm:** generate matrix YAML fragment and Markdown table; `--check` regenerates and compares; reference date passed explicitly (`--today`) so the check is reproducible.
- **Errors:** `matrix_stale`, `version_eol`.
- **Determinism:** D0 given the reference date.
- **Performance budget:** negligible.
- **Crate:** `fid-py-gen`

## Test fixtures

Sample `python-versions.json` files under `fixtures/py-versions/` (project-authored).

## Build prompt

> In crate `fid-py-gen`, add `cargo xtask py-matrix [--check] [--today YYYY-MM-DD]`. Read `python-versions.json` (version, supported_from, eol). Generate the CI matrix fragment and `docs/python-support.md`. With `--check`, regenerate and fail with `matrix_stale` if either generated file differs, and fail with `version_eol` if any listed version's eol is before `--today`. Acceptance: the three cases in the spec. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; no clock reads (date passed in).

## Open questions

- The initial version list is for the maintainers to set at the first Python release; the AC values are examples.

## Children

_Leaf: none._
