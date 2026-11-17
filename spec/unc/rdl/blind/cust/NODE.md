---
id: UNC.RDL.BLIND.CUST
title: Custody model, repository guard and sealed CI job
level: 4
parent: UNC.RDL.BLIND
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.RDL.BLIND-001, REQ-UNC.RDL.BLIND-002]
covers: []
depends_on: []
uses_artifacts: [UNC.RDL.FORMAT, UNC.RDL.AST, UNC.RDL.GEN.CMD, KRN.MAN.WASM]
side_effect: R1
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.RDL.BLIND.CUST: Custody model, repository guard and sealed CI job

## Purpose

Keeps blind (tier B) data out of reach of everyone who writes kernel code, people and agents alike. A custodian role holds sealed seeds outside the main repository. A sealed CI job regenerates the blind set from those seeds, runs it against the kernel artifact with the self-test runner, and publishes only per-aim aggregates. A repository guard fails any commit that adds tier B content or seeds.

## Scope

**In:**
- Custody procedure (role-based): custodian holds seeds in a separate private store; seeds enter the sealed job only as a CI secret; the main repository holds tier B ID, aim and digest only
- Sealed job: generate with `fid rdl generate-dataset` from sealed seeds, check digests against the committed tier B digests, run `fid rdl run-selftest --dry-run` against the WASM kernel artifact, aggregate, publish
- Aggregate report `blind-stats/v1`: per aim, case count, pass count, max(|d| + U_d), optional signed mean deviation; nothing per case
- Repository guard: CI check that rejects tier B records with content, seed-like files matching the custody naming rule, or any blob whose digest is a tier B input digest

**Out:**
- Generators and runner (used as built binary artifacts)
- Rotation (owned by `UNC.RDL.BLIND.ROT`)
- Naming the custodian (maintainers)

## Requirements

- **REQ-UNC.RDL.BLIND.CUST-001** The repository guard shall fail CI if a commit adds a tier B record with input or reference content, a file under the custody seed naming rule, or a blob whose digest equals a tier B input digest.
- **REQ-UNC.RDL.BLIND.CUST-002** The sealed job shall publish only `blind-stats/v1` aggregates per aim, and a schema check shall reject any report containing case IDs, inputs, references or per-case deviations.
- **REQ-UNC.RDL.BLIND.CUST-003** If a regenerated case's digest differs from the committed tier B digest, then the sealed job shall fail with `blind_digest_mismatch` and publish no statistics.
- **REQ-UNC.RDL.BLIND.CUST-004** The sealed job's logs shall contain no per-case values, verified by a sentinel case whose input digest is scanned for in logs.

## Acceptance criteria

- **AC-1** Given a test custody setup with sealed seeds in a CI secret and `fixtures/rdl/blind/cust/committed_tier_b.json`, when the sealed job runs, then `blind-stats/v1` lists per-aim counts and max(|d| + U_d) and passes the schema check.
- **AC-2** (error path) Given a commit adding `fixtures/rdl/blind/cust/leak_record.json` (tier B with inputs), then the guard fails naming the file.
- **AC-3** (error path) Given a tampered seed, then `blind_digest_mismatch` and no statistics.
- **AC-4** Given the sentinel case, then its digest and values do not appear in the job logs.

## Design

- **Registry command(s):** none (tooling: CI workflow plus a guard script under `tools/rdl/`).
- **Data structures:** `blind-stats/v1`; custody naming rule `*.rdlseed`.
- **Algorithm:** as in Scope; aggregation in the job; the guard loads committed tier B digests.
- **Errors:** `blind_digest_mismatch`, `blind_report_leak`, `blind_content_committed`.
- **Determinism:** D1 (depends on the kernel artifact under test).
- **Performance budget:** sealed job under 30 minutes [unverified].
- **Crate:** none (tooling; consumes the `fid` binary and WASM kernel as artifacts).

## Test fixtures

`fixtures/rdl/blind/cust/`: a test custody set with throwaway seeds that are not real blind data (CC-BY-4.0).

## Build prompt

> Add tooling under `tools/rdl/` and a CI workflow: (1) a repository guard that fails if a commit adds a tier B `rdl-dataset/v1` record with content, a `*.rdlseed` file, or a blob whose digest equals a committed tier B input digest; (2) a sealed job that receives seeds from a CI secret, regenerates the set with the built `fid rdl generate-dataset`, checks every digest against the committed tier B digests (`blind_digest_mismatch`), runs `fid rdl run-selftest --dry-run` against the WASM kernel artifact, and publishes only `blind-stats/v1` (per aim: cases, passes, max(|d| + U_d), optional signed mean), validated by a schema that rejects any per-case data; logs must not contain per-case values (sentinel test). Tests AC-1..AC-4 with a throwaway custody set. Tooling reads artifacts only.

## Open questions

- Custodian identity (maintainer decision before first sealing).
- Whether a per-aim signed mean leaks usable information is [unverified].

## Children

_Leaf: none._
