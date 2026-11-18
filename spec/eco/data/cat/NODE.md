---
id: ECO.DATA.CAT
title: Dataset catalogue and admission check
level: 3
parent: ECO.DATA
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-data]
built_pr: 60
satisfies: [REQ-ECO.DATA-001, REQ-ECO.DATA-004]
covers: []
depends_on: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.DATA.CAT: Dataset catalogue and admission check

## Purpose

One catalogue file listing every public dataset the project uses for tests, benchmarks and examples, with its source, licence, size, digest, owning domain and redistribution status, plus the check that refuses an entry whose licence does not allow mirroring unless it is marked fetch-only, and refuses entries that declare standards text or unreleased customer data.

## Scope

**In:**
- `datasets.toml` schema and the admission policy as data (redistributable licence IDs)
- `cargo xtask datasets-check`

**Out:**
- Downloading and verifying files (owned by `ECO.DATA.FETCH`)
- Which datasets a domain needs and their expected results (owned by `IO`, `KRN`, `UNC`, `SCAN`, `IND` and others)
- Dependency licence allowlist (owned by `ECO.RULES.SPDX`)

## Requirements

- **REQ-ECO.DATA.CAT-001** Each catalogue entry shall have `id` (`^[a-z0-9][a-z0-9._-]{2,63}$`, unique), `title`, `source` (URL), `licence` (SPDX expression or `LicenseRef-<name>` with a `terms_url`), `size_bytes`, `sha256` (64 lowercase hex), `owner` (a level-1 domain ID), `redistribution` (`mirror` or `fetch-only`) and at least one URL in `urls`.
- **REQ-ECO.DATA.CAT-002** If an entry has `redistribution = "mirror"` and its licence expression cannot be satisfied using only the policy's redistributable licence IDs, then the check shall fail.
- **REQ-ECO.DATA.CAT-003** If an entry declares `contains_standards_text = true`, or declares `contains_customer_data = true` without a `release_ref`, then the check shall fail.
- **REQ-ECO.DATA.CAT-004** If an entry's `owner` is not a level-1 domain ID in the spec tree list supplied to the check, then the check shall fail.
- **REQ-ECO.DATA.CAT-005** The check shall run offline and print findings sorted by entry ID and code.

## Acceptance criteria

- **AC-1** Given `fixtures/datasets/valid.toml` (three entries: one CC-BY-4.0 mirror, one `LicenseRef-NIST-Terms` fetch-only, one CC0-1.0 mirror) and `fixtures/datasets/domains.txt`, when `cargo xtask datasets-check --catalogue fixtures/datasets/valid.toml --domains fixtures/datasets/domains.txt` runs, then it exits 0.
- **AC-2** (error path) Given `mirror-noredist.toml` (a `LicenseRef-Vendor-EULA` entry marked mirror), when the check runs, then it exits 1 with `DATA-REDIST <id>`.
- **AC-3** (error path) Given `standards.toml` (an entry with `contains_standards_text = true`), when the check runs, then it exits 1 with `DATA-STANDARDS <id>`.
- **AC-4** (error path) Given `bad-digest.toml` (63-char digest) and `dup.toml` (duplicate id), when the check runs on each, then it exits 1 with `DATA-SCHEMA` and `DATA-DUP` respectively.
- **AC-5** (error path) Given `bad-owner.toml` (owner `XYZ`), when the check runs, then it exits 1 with `DATA-OWNER`.

## Design

- **Registry command(s):** none; repository tool `cargo xtask datasets-check --catalogue <toml> --domains <file>`.
- **Data structures:** `[policy] redistributable = ["CC0-1.0", "CC-BY-4.0", "CC-BY-SA-4.0", "MIT", "Apache-2.0", "BSD-3-Clause", "ODbL-1.0", "PDDL-1.0"]` (proposal; maintainers confirm in review); `[[dataset]]` fields per REQ-001 plus optional `notes`, `contains_standards_text` (default false), `contains_customer_data` (default false), `release_ref`.
- **Algorithm:** bounded TOML parse (1 MiB); validate; SPDX satisfiability against the redistributable list (same semantics as ECO.CI.LIC: `OR` any, `AND` all; `LicenseRef-*` is never redistributable unless listed).
- **Errors:** `DATA-SCHEMA`, `DATA-DUP`, `DATA-REDIST`, `DATA-STANDARDS`, `DATA-CUSTOMER`, `DATA-OWNER`, `DATA-IO`.
- **Determinism:** D0.
- **Performance budget:** under 1 s for 1 000 entries [unverified].
- **Crate:** `fid-data` (dataset catalogue and fetch library, with the xtask subcommand as a thin wrapper).

## Test fixtures

Hand-written TOML catalogues and a domain list under `fixtures/datasets/` (MIT OR Apache-2.0). No real dataset files are needed.

## Build prompt

> In the implementation repository, create crate `fid-data` with a catalogue module and expose `cargo xtask datasets-check --catalogue <toml> --domains <file>`. Schema: `[policy] redistributable = [...]`; `[[dataset]] id, title, source, licence, terms_url (required for LicenseRef-), size_bytes, sha256, owner, redistribution = mirror|fetch-only, urls = [...], contains_standards_text = false, contains_customer_data = false, release_ref?`. Checks: schema and id regex `^[a-z0-9][a-z0-9._-]{2,63}$`; unique ids; 64-hex lowercase digest; owner in the domain list; mirror entries need a licence satisfiable from `redistributable` (OR any, AND all, LicenseRef never unless listed); fail on standards text, and on customer data without `release_ref`. Findings sorted by id then code, exit 0/1, offline, bounded input (1 MiB). Fixtures and tests for AC-1..AC-5 of spec node ECO.DATA.CAT. No `unwrap`/`expect`/`panic`/`unsafe` in non-test code. Seed `datasets.toml` empty except the policy table.

## Open questions

- The redistributable list is a proposal; ODbL and share-alike licences may need a maintainer call for mirrored data [unverified compatibility with CC-BY-4.0 publication of results].

## Children

_Leaf: none._
