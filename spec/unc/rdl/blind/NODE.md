---
id: UNC.RDL.BLIND
title: Tier B blind sets and challenge handling
level: 3
parent: UNC.RDL
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.RDL-002]
covers: []
depends_on: [UNC.RDL.FORMAT, UNC.RDL.AST]
cli: [fid rdl submit-challenge]
mcp: [rdl_submit_challenge]
journal: [rdl.submit_challenge@1]
updated: 2026-11-16
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

- Who the external custodian is (a founding expert or an escrow service) is for maintainers to decide; this may need a decision record at P1b.
- The side-effect class of `rdl.submit_challenge@1` (R1 locally, X1 if served from a hosted verifier) depends on where it runs.

## Children

_None yet: status `open`._
