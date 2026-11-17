---
id: ECO.CI.LIC
title: Dependency licence and attribution check
level: 3
parent: ECO.CI
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.CI-003]
covers: []
depends_on: [ECO.RULES.SPDX]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.CI.LIC: Dependency licence and attribution check

## Purpose

Checks every dependency's declared licence against the project's SPDX allowlist (content owned by ECO.RULES.SPDX, under D-001), and checks that every third-party asset in the repository has an `ATTRIBUTION.md` entry (CONSTITUTION §8). It owns the evaluation logic; the list of allowed licences is data it reads.

## Scope

**In:**
- Reading dependency licences from the build tool's package metadata
- Evaluating SPDX expressions (`AND`, `OR`, `WITH`, parentheses) against the allowlist
- Clarifications for packages with a missing or non-standard licence field, pinned by licence-file digest
- Asset attribution: every file under configured asset paths has a row in `ATTRIBUTION.md`

**Out:**
- Which licences are allowed (owned by `ECO.RULES.SPDX`)
- Plugin licence rules (owned by `ECO.SDK`)
- Dataset licences (owned by `ECO.DATA.CAT`)
- SBOM generation for releases (owned by `ECO.REL`)

## Requirements

- **REQ-ECO.CI.LIC-001** If a dependency's SPDX expression cannot be satisfied using only allowlisted licence IDs (an `OR` needs one allowed branch, an `AND` needs all), then the check shall fail and name the package, version and expression.
- **REQ-ECO.CI.LIC-002** If a dependency has no licence expression and no clarification entry whose recorded SHA-256 matches its licence file, then the check shall fail.
- **REQ-ECO.CI.LIC-003** If a licence `WITH` exception ID is not in the allowlist's exceptions list, then the check shall fail.
- **REQ-ECO.CI.LIC-004** If a file under a configured asset path has no row in `ATTRIBUTION.md` naming its path, or a row names a file that does not exist, then the check shall fail and name the path.
- **REQ-ECO.CI.LIC-005** The check shall run offline from a metadata file and the repository tree, and its output shall be sorted and byte-stable.

## Acceptance criteria

- **AC-1** Given `fixtures/lic/metadata-ok.json` (packages under `MIT`, `Apache-2.0`, `MIT OR Apache-2.0`), allowlist `fixtures/lic/allow.toml` and an asset tree with complete `ATTRIBUTION.md`, when `cargo xtask licence-check --metadata fixtures/lic/metadata-ok.json --allow fixtures/lic/allow.toml --root fixtures/lic/tree-ok` runs, then it exits 0.
- **AC-2** (error path) Given `metadata-gpl.json` with a package `GPL-3.0-only`, when the check runs, then it exits 1 with `LIC-DENIED <name> <version> GPL-3.0-only`.
- **AC-3** Given `metadata-or.json` with `GPL-3.0-only OR MIT`, when the check runs, then it exits 0; given `metadata-and.json` with `MIT AND GPL-3.0-only`, it exits 1.
- **AC-4** (error path) Given `metadata-nolicence.json` with no licence field and no clarification, when the check runs, then it exits 1 with `LIC-MISSING`; with clarification `fixtures/lic/clarify.toml` whose digest matches, it exits 0; with a mismatching digest, it exits 1 with `LIC-CLARIFY-STALE`.
- **AC-5** (error path) Given `tree-missing-attr/` (an icon file without an `ATTRIBUTION.md` row), when the check runs, then it exits 1 with `ATTR-MISSING assets/icons/x.svg`.
- **AC-6** (error path) Given `metadata-with.json` with `Apache-2.0 WITH LLVM-exception` and the exception not allowlisted, when the check runs, then it exits 1 with `LIC-EXCEPTION`.

## Design

- **Registry command(s):** none; repository tool `cargo xtask licence-check --metadata <json> --allow <toml> --root <dir> [--clarify <toml>]`. In CI, the metadata file is produced by `cargo metadata --format-version 1 --locked --offline`.
- **Data structures:** allowlist (from ECO.RULES.SPDX): `licences = [...]`, `exceptions = [...]`, `asset_licences = [...]`, `asset_paths = [...]`; clarification: `[[clarify]] package, version, expression, licence_file_sha256`.
- **Algorithm:** parse SPDX expressions with a bounded recursive-descent parser (max depth 16, max 256 tokens); satisfiability: leaf true if ID allowed, `AND` all, `OR` any, `WITH` needs both IDs allowed; treat `+` suffix and deprecated IDs as their base ID only if the allowlist lists the base [unverified for every deprecated ID]. For attribution, parse the `ATTRIBUTION.md` table's first column as repo-relative paths (glob rows allowed) and compare with files under `asset_paths`.
- **Errors:** `LIC-DENIED`, `LIC-MISSING`, `LIC-CLARIFY-STALE`, `LIC-EXCEPTION`, `LIC-PARSE`, `ATTR-MISSING`, `ATTR-ORPHAN`.
- **Determinism:** D0.
- **Performance budget:** under 2 s for 1 000 packages [unverified until measured].
- **Crate:** `fid-xtask`.

## Test fixtures

Synthetic `cargo metadata`-shaped JSON files, allowlists and small asset trees under `fixtures/lic/` (MIT OR Apache-2.0). No real third-party licence texts are needed except one short licence file used for the digest test, which must itself be under an allowlisted licence.

## Build prompt

> In the implementation repository, add `licence-check` to `fid-xtask`. Inputs: `--metadata` (JSON in `cargo metadata --format-version 1` shape; read `packages[].name`, `version`, `license`, `license_file`, `source`; skip workspace members), `--allow` (TOML: `licences`, `exceptions`, `asset_licences`, `asset_paths`), `--root` (repo tree containing `ATTRIBUTION.md`), optional `--clarify` (`[[clarify]] package, version, expression, licence_file_sha256`). Implement a bounded SPDX expression parser (AND, OR, WITH, parentheses; depth ≤ 16, ≤ 256 tokens) and satisfiability against the allowlist. Findings `LIC-DENIED`, `LIC-MISSING`, `LIC-CLARIFY-STALE`, `LIC-EXCEPTION`, `LIC-PARSE`, `ATTR-MISSING`, `ATTR-ORPHAN`, sorted; exit 0/1; offline. Write fixtures under `fixtures/lic/` and tests for AC-1..AC-6 of spec node ECO.CI.LIC. No `unwrap`/`expect`/`panic`/`unsafe` in non-test code. Do not embed any licence list in code: the allowlist file is the only source.

## Open questions

- Whether to wrap an existing tool (for example cargo-deny) instead of writing the evaluator; acceptable if it meets every acceptance criterion and is itself on the allowlist [unverified].

## Children

_Leaf: none._
