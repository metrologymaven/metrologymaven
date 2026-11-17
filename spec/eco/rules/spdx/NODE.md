---
id: ECO.RULES.SPDX
title: SPDX licence allowlist for dependencies and assets
level: 3
parent: ECO.RULES
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-ECO.RULES-001]
covers: []
depends_on: [ECO.RULES.SRC]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.RULES.SPDX: SPDX licence allowlist for dependencies and assets

## Purpose

The project's list of licences that dependencies and third-party assets may carry, written as data in the canonical rules source, as D-001 requires ("a dependency SPDX allowlist is to be written in ECO.RULES"). ECO.CI.LIC evaluates dependencies and assets against it; the list itself, and every change to it, is reviewed here as a house rule. The level-2 review assigned ownership of this list to ECO.RULES.

## Scope

**In:**
- `house-rules/spdx-allowlist.toml`: allowed licence IDs for dependencies, allowed exception IDs, allowed asset licences, asset paths, and a `basis` note per entry
- The change rule for the list
- A validation test for the file

**Out:**
- Evaluating dependencies and assets against it (owned by `ECO.CI.LIC`)
- Dataset redistribution policy (owned by `ECO.DATA.CAT`)
- Plugin licence rules (owned by `ECO.SDK`)
- The project's own licence (D-001)

## Requirements

- **REQ-ECO.RULES.SPDX-001** The allowlist shall contain, for dependencies, only permissive licences compatible with distributing the project under MIT OR Apache-2.0 (D-001), and every entry shall have a `basis` note.
- **REQ-ECO.RULES.SPDX-002** The allowlist shall not contain any GPL, LGPL, AGPL, SSPL or non-commercial licence ID.
- **REQ-ECO.RULES.SPDX-003** The asset section shall allow CC-BY-4.0 and the other asset licences listed under Design, so that `ATTRIBUTION.md` entries can satisfy CONSTITUTION §8 and OUTR.BRAND's asset rule.
- **REQ-ECO.RULES.SPDX-004** Every change to the allowlist shall be made in its own PR, approved by a maintainer other than its author, and the PR description shall state the reason.
- **REQ-ECO.RULES.SPDX-005** The file shall parse with the schema ECO.CI.LIC reads (`licences`, `exceptions`, `asset_licences`, `asset_paths`).

## Acceptance criteria

- **AC-1** Given `house-rules/spdx-allowlist.toml`, when `tools/tests/test_spdx_allowlist.py` runs, then the file parses and has the four keys of REQ-005, and every entry in `licences` has a matching `basis` entry.
- **AC-2** The same test fails if any ID in `licences` or `asset_licences` matches `^(A|L)?GPL-|^SSPL-|-NC-|^CC-BY-NC` (checked against fixture `fixtures/spdx/bad-gpl.toml`, which must make the test fail).
- **AC-3** `asset_licences` contains `CC-BY-4.0`, `CC0-1.0` and `OFL-1.1`.
- **AC-4** `licences` contains `MIT` and `Apache-2.0` (the project's own licences, so first-party crates pass the check).

## Design

- **Registry command(s):** none; data file plus a test.
- **Data structures:** proposed initial content (maintainers confirm in review):
  - `licences`: `MIT`, `Apache-2.0`, `BSD-2-Clause`, `BSD-3-Clause`, `ISC`, `Zlib`, `0BSD`, `MIT-0`, `BSL-1.0`, `CC0-1.0`, `Unicode-3.0`, `Unicode-DFS-2016` [unverified that each is needed by the eventual dependency set];
  - `exceptions`: `LLVM-exception`;
  - `asset_licences`: `CC0-1.0`, `CC-BY-4.0`, `OFL-1.1`, `MIT`, `Apache-2.0`;
  - `asset_paths`: `assets/**`, `crates/*/assets/**`;
  - `[basis]` table: one short reason per ID.
- **Algorithm:** n/a.
- **Errors:** test failures only.
- **Determinism:** D0.
- **Performance budget:** n/a.
- **Crate:** not Rust; `house-rules/spdx-allowlist.toml` and a stdlib `unittest` in `tools/tests/`.

## Test fixtures

`fixtures/spdx/bad-gpl.toml` (a copy of the list with `GPL-3.0-only` added) to prove the guard test fails.

## Build prompt

> In the spec repository (canonical rules source), create `house-rules/spdx-allowlist.toml` with keys `licences`, `exceptions`, `asset_licences`, `asset_paths` and a `[basis]` table, seeded with the content listed in spec node ECO.RULES.SPDX (Design). Add `tools/tests/test_spdx_allowlist.py` (stdlib `unittest`; parse with `tomllib`, skip with a clear message on Python < 3.11 unless a JSON twin exists) implementing AC-1..AC-4, plus a negative test using `fixtures/spdx/bad-gpl.toml`. Add the file to `house-rules/manifest.toml` as a data file distributed with the rules. Do not add copyleft licences.

## Open questions

- **MPL-2.0** (file-level copyleft) is common in the Rust ecosystem and is left out of the proposal; adding it is a maintainer call under D-001 [unverified how many likely dependencies need it].
- Whether plugins listed by the project may use other licences is ECO.SDK's question, not this list's.

## Children

_Leaf: none._
