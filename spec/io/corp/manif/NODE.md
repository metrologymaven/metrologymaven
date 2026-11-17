---
id: IO.CORP.MANIF
title: Corpus manifest format, licence checks and fetching
level: 3
parent: IO.CORP
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-IO.CORP-001, REQ-IO.CORP-002]
covers: []
depends_on: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# IO.CORP.MANIF: Corpus manifest format, licence checks and fetching

## Purpose

Defines the per-format corpus manifest (what files exist, where they come from, under which licence, their digest and the expected outcome) and the CI tool that validates manifests, enforces the redistribution rule and fetches external files by URL and digest.

## Scope

**In:**
- Manifest schema (`corpus/<format-id>.toml`): entries with `path` or `url`, `source`, `licence` (SPDX ID or `unknown`), `sha256`, `expect` (`accept{counts}` or `reject{error}`), `tags` (e.g. `hostile`, `p0-subset`)
- Validator: schema, unique digests, licence rule, every registered first-party format has a manifest
- Fetcher: downloads `url` entries into a cache, verifies digest, fails on mismatch
- Redistribution rule: in-repository files only with an SPDX licence on the redistribution allowlist

**Out:**
- Hostile file generation (`IO.CORP.HOSTILE`), scoring (`IO.CORP.SCORE`)

## Requirements

- **REQ-IO.CORP.MANIF-001** Every registered first-party format shall have a manifest whose entries each list source, licence (SPDX ID or `unknown`), SHA-256 and expected outcome, and the validator shall fail with `manifest_missing` for a format without one.
- **REQ-IO.CORP.MANIF-002** If an entry stored in the repository has licence `unknown` or a licence not on the redistribution allowlist, then the validator shall fail with `licence_not_redistributable` naming the entry.
- **REQ-IO.CORP.MANIF-003** When the fetcher downloads a `url` entry, it shall verify the SHA-256 and fail with `digest_mismatch` if it differs.
- **REQ-IO.CORP.MANIF-004** If a committed file's SHA-256 differs from its manifest entry, then the validator shall fail with `digest_mismatch`.

## Acceptance criteria

- **AC-1** Given `fixtures/io/corp/manif/ok/` (two manifests, three files, one `url` entry served by a local test HTTP server), when `corpus-check` runs with registry formats `mesh.stl` and `mesh.ply`, then it exits 0.
- **AC-2** (error path) Given a registry that also lists `mesh.obj` with no manifest, then it fails `manifest_missing{mesh.obj}`.
- **AC-3** (error path) Given `fixtures/io/corp/manif/bad_licence/` (in-repo file with licence `unknown`), then fails `licence_not_redistributable`.
- **AC-4** (error path) Given the test server returning altered bytes, then fetch fails `digest_mismatch` and the cache holds no file for that entry.
- **AC-5** (error path) Given an in-repo file edited after its digest was recorded, then fails `digest_mismatch`.

## Design

- **Registry commands:** none (CI tool `corpus-check`, binary in crate `fid-io-corp`).
- **Data structures:** `Manifest { format: FormatId, entries: [Entry] }`; `Entry { path?|url?, source, licence, sha256, expect, tags }`; `Expect::Accept{counts: BTreeMap<String,u64>} | Reject{error: String}`.
- **Algorithm:** parse TOML; validate; read the registry listing (from `import.list_formats@1` output JSON) to find formats; hash files; fetch with a size cap from the `IO.REG.LIMITS` defaults.
- **Errors:** `manifest_missing`, `licence_not_redistributable`, `digest_mismatch`, `malformed`.
- **Determinism:** D0 (network fetch outcome aside, which is `N` and fails closed).
- **Performance budget:** trivial.
- **Crate:** `fid-io-corp`.

## Test fixtures

Small synthetic manifests and files under `fixtures/io/corp/manif/` (CC-BY-4.0); a local HTTP test server in the test harness. The redistribution allowlist starts as `CC0-1.0, CC-BY-4.0, MIT, Apache-2.0, BSL-1.0, BSD-3-Clause` [unverified: to align with ECO.RULES under D-001].

## Build prompt

> In crate `fid-io-corp`, add CI binary `corpus-check`: parse per-format manifests `corpus/<format-id>.toml` (entries: `path` or `url`, `source`, `licence` SPDX or `unknown`, `sha256`, `expect` accept-with-counts or reject-with-error, `tags`); fail `manifest_missing` for any registered first-party format (from `fid import list-formats --json`) without a manifest; fail `licence_not_redistributable` for in-repo files with `unknown` or non-allowlisted licences; verify digests of committed files and fetched URLs (`digest_mismatch`, no partial cache files). BTreeMap only; no panics, no unwrap (CONSTITUTION §8). Tests AC-1..AC-5.

## Open questions

- The redistribution allowlist should come from ECO.RULES once written.

## Children

_Leaf: none._
