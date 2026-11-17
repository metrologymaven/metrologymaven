---
id: UNC.RDL.BLIND
title: Tier B blind sets and challenge handling
level: 3
parent: UNC.RDL
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.RDL-002]
covers: []
depends_on: [UNC.RDL.FORMAT]
uses_artifacts: [UNC.RDL.AST, KRN.MAN.WASM]
cli: [fid rdl submit-challenge]
mcp: [rdl_submit_challenge]
journal: [rdl.submit_challenge@1]
updated: 2026-11-17
---

# UNC.RDL.BLIND: Tier B blind sets and challenge handling

## Purpose

Anti-overfitting for a code base written partly by agents that can see test data. Blind sets are generated from sealed seeds held by an external custodian, never stored in the main repository, and run in a sealed CI job that reports only pass or fail and deviation statistics. A challenge path lets any verifier submit points and receive kernel results while the kernel never sees the verifier's reference.

## Scope

**In:**
- Custody model: sealed seeds, custodian role, sealed CI job reporting only aggregate statistics
- Tier B records in the main repository as ID, aim and digest only
- `rdl.submit_challenge@1`: accept points for an aim, return kernel results, never accept a reference value
- Rotation of blind sets and disclosure after rotation

**Out:**
- Generators (owned by `UNC.RDL.GEN`)
- Reviewer canaries in attestation packets (owned by `TRUST` or `ECO`)

## Requirements

- **REQ-UNC.RDL.BLIND-001** Tier B dataset content and seeds shall never be committed to the main repository or included in any public release; only ID, aim and digest may be.
- **REQ-UNC.RDL.BLIND-002** The sealed CI job shall report only per-aim pass counts and maximum |d| + U_d, never per-case inputs or references.
- **REQ-UNC.RDL.BLIND-003** `rdl.submit_challenge@1` shall accept input points and an aim and return kernel results, and shall reject any submission that contains a reference value field.
- **REQ-UNC.RDL.BLIND-004** When a blind set is rotated, the previous set may be disclosed as tier G, and its results history shall be kept.

## Interfaces

- **Offers:** `rdl.submit_challenge@1`; sealed-job statistics for the conformance dashboard.
- **Consumes:** record format (UNC.RDL.FORMAT); runner (UNC.RDL.AST).

## Decomposition hints

CUSTODY (process and sealed job definition), CHALLENGE (command), ROTATE.

## Open questions

- Who the external custodian is (a founding expert or an escrow service) is for maintainers to decide. The custody model in `UNC.RDL.BLIND.CUST` is written against a custodian role and does not depend on that choice. A decision record is needed before the first blind set is sealed (P1b), not to write the specs.
- **Applied (side-effect class):** `rdl.submit_challenge@1` is R1 and runs locally on the challenger's own machine with their own kernel. A hosted verifier would be an ENT service that wraps it and is out of scope here.
- **OUTR request (AST run records):** OUTR asked that `UNC.RDL.AST` run records carry the signed deviation and UEC level per output. The sealed job (`UNC.RDL.BLIND.CUST`) reads AST records as artifacts but publishes only per-aim aggregates. Signed per-case deviations must never leave the sealed job, because they would leak the blind references. A per-aim signed bias summary is acceptable [unverified: leakage analysis]. The AST change itself belongs to `UNC.RDL.AST`.
- Dependencies converted (D-034): the sealed job reads the AST runner and the WASM kernel as artifacts (tooling), not as crate dependencies.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| UNC.RDL.BLIND.CUST | Custody model, repository guard and sealed CI job (leaf) | 001 002 | none |
| UNC.RDL.BLIND.CHAL | The rdl.submit_challenge@1 command (leaf) | 003 | none |
| UNC.RDL.BLIND.ROT | Rotation and disclosure as tier G (leaf) | 004 | none |
