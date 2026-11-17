---
id: KRN.SOLV.CONIC.VEND
title: Vendored solver tree, source digest and audit record check
level: 4
parent: KRN.SOLV.CONIC
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-KRN.SOLV.CONIC-001, REQ-KRN.SOLV.CONIC-002, REQ-KRN.SOLV.CONIC-006]
covers: []
depends_on: []
uses_artifacts: [KRN.NUM.LINT.FIND, ECO.RULES.SPDX]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.SOLV.CONIC.VEND: Vendored solver tree, source digest and audit record check

## Purpose

Makes the vendored conic solver auditable. It fixes the repository layout (pristine upstream snapshot, numbered patch files, patched crate), defines the audit record `AUDIT.toml`, and provides the CI check that the patched crate equals upstream plus the listed patches, that the upstream digest and licence are recorded, that the KNP-1 lint report for the crate is clean apart from listed waivers, and that an upstream update carries every earlier patch forward or justifies dropping it.

## Scope

**In:**
- Layout under `vendor/fid-clarabel/`: `upstream/` (pristine tree at the pinned commit), `patches/NNNN-<slug>.patch`, `crate/` (the patched crate built into the kernel), `AUDIT.toml`
- `AUDIT.toml` schema v1: upstream repository URL, commit, tree digest, SPDX licence, patch list (id, file, KNP-1 rule IDs addressed, rationale), waiver list (references to `knp1-waivers.toml` entries with justification), the digest of the lint report it was audited with, auditor and date
- Tree digest: SHA-256 over the sorted list of (relative path, SHA-256 of content) of `upstream/`
- `cargo xtask conic-audit [--previous <AUDIT.toml>]` CI check

**Out** (and which node owns it instead):
- Writing the patches (`KRN.SOLV.CONIC.PATCH`)
- The lint engine and report schema (`KRN.NUM.LINT`, report from `KRN.NUM.LINT.FIND`)
- The SPDX allowlist content (`ECO.RULES.SPDX`)

## Requirements

- **REQ-KRN.SOLV.CONIC.VEND-001** The audit check shall recompute the tree digest of `upstream/` and shall fail with `AUDIT-DIGEST` if it differs from `AUDIT.toml`.
- **REQ-KRN.SOLV.CONIC.VEND-002** The audit check shall apply the patches in numeric order to a copy of `upstream/` and shall fail with `AUDIT-PATCH-DRIFT` if the result differs from `crate/` in any file.
- **REQ-KRN.SOLV.CONIC.VEND-003** The audit check shall fail with `AUDIT-LICENCE` if the recorded licence is not allowed by `house-rules/spdx-allowlist.toml`.
- **REQ-KRN.SOLV.CONIC.VEND-004** The audit check shall read the KNP-1 lint report and shall fail with `AUDIT-LINT` if any unwaived finding lies in `crate/` or any waived finding there is not listed in `AUDIT.toml` with a justification.
- **REQ-KRN.SOLV.CONIC.VEND-005** When `--previous` names the audit record at the merge base and the upstream commit differs, the check shall fail with `AUDIT-PATCH-MISSING` for every earlier patch ID that is neither present nor listed under `dropped` with a justification.
- **REQ-KRN.SOLV.CONIC.VEND-006** Every patch listed in `AUDIT.toml` shall name at least one KNP-1 rule ID or the reason class `scope-reduction`, and the check shall fail with `AUDIT-SCHEMA` otherwise.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/solv/vend/ok/` (a tiny upstream tree, two patches, matching crate, clean lint report, `MIT` licence), when `cargo xtask conic-audit` runs, then it exits 0.
- **AC-2** (error path) Given the same with one byte changed in `crate/src/lib.rs`, then it exits 1 with `AUDIT-PATCH-DRIFT` naming the file.
- **AC-3** (error path) Given one byte changed in `upstream/`, then `AUDIT-DIGEST`.
- **AC-4** (update) Given `--previous` with patches 0001 and 0002 and a new record with only 0001 and no `dropped` entry, then `AUDIT-PATCH-MISSING 0002`; with `dropped = [{ id = "0002", justification = "fixed upstream" }]`, then exit 0.
- **AC-5** (lint) Given a lint report with an unwaived `KNP1-POW` finding in `crate/src/x.rs`, then `AUDIT-LINT`.

## Design

- **Registry command(s):** none; repository tool `cargo xtask conic-audit` (tooling layer, not a registry command).
- **Data structures:** `AuditRecord { schema, upstream: { url, commit, tree_sha256, licence }, patches: [{ id, file, rules, rationale }], dropped: [{ id, justification }], waivers: [{ waiver_ref, justification }], lint_report_sha256, auditor, date }`.
- **Algorithm:** sorted directory walks; patch application with a strict unified-diff applier (no fuzz); file-by-file comparison; lint report parsed with the `KRN.NUM.LINT.FIND` schema; SPDX evaluation shared with the lint's expression parser if exposed, else a local copy of the same grammar.
- **Errors:** `AUDIT-DIGEST`, `AUDIT-PATCH-DRIFT`, `AUDIT-LICENCE`, `AUDIT-LINT`, `AUDIT-PATCH-MISSING`, `AUDIT-SCHEMA`.
- **Determinism:** D0.
- **Performance budget:** < 10 s for a 20 000-line upstream tree [unverified until measured].
- **Crate:** `fid-xtask` (tooling layer).

## Test fixtures

`fixtures/solv/vend/`: miniature upstream trees, patches, lint reports and audit records for each case; project licence.

## Build prompt

> In `fid-xtask`, add `conic-audit [--previous <AUDIT.toml>]` over `vendor/fid-clarabel/` (`upstream/`, `patches/NNNN-*.patch`, `crate/`, `AUDIT.toml`). Checks: tree digest of `upstream/` (SHA-256 over sorted (path, content SHA-256) pairs) equals the record (`AUDIT-DIGEST`); applying patches in order to a copy of `upstream/` with a strict no-fuzz applier yields exactly `crate/` (`AUDIT-PATCH-DRIFT`); recorded licence allowed by `house-rules/spdx-allowlist.toml` (`AUDIT-LICENCE`); the `knp1-findings.json` report has no unwaived finding in `crate/` and every waived one is justified in the record (`AUDIT-LINT`); with `--previous`, every earlier patch is present or listed in `dropped` with a justification (`AUDIT-PATCH-MISSING`); schema fields present and each patch names KNP-1 rule IDs or `scope-reduction` (`AUDIT-SCHEMA`). Exit 0/1, sorted output, offline. Tests: `fixtures/solv/vend/` for AC-1..AC-5 of spec node KRN.SOLV.CONIC.VEND. Constraints (CONSTITUTION §8): no `unsafe`, deny `unwrap`/`expect`/`panic`, bounded parsers.

## Open questions

- Clarabel's licence (Apache-2.0 [unverified]) must be on the SPDX allowlist before the first audit can pass.
- A vendored workspace crate is not a third-party package, so `ECO.REPO.ALLOW` does not list it; this audit is its gate instead. Confirm with ECO.REPO that vendored kernel crates are covered by this record rather than the allowlist.

## Children

_Leaf: none._
