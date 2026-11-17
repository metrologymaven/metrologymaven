---
id: UNC.RDL.BLIND.ROT
title: Rotation and disclosure as tier G
level: 4
parent: UNC.RDL.BLIND
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-UNC.RDL.BLIND-004]
covers: []
depends_on: [UNC.RDL.FORMAT]
uses_artifacts: [UNC.RDL.GEN.CMD]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.RDL.BLIND.ROT: Rotation and disclosure as tier G

## Purpose

When a blind set is retired, the custodian publishes its seeds. A maintainer script regenerates the set, proves that every regenerated record matches the committed tier B digest, and converts the records to tier G with a disclosure note. The sealed-job statistics history of the retired set is kept unchanged.

## Scope

**In:**
- `disclose(tier_b_index, seeds) -> Vec<RdlDataset>`: regenerate, verify digests, re-identify as tier G (`rdl:G/<aim>/blind-<set>/<nnnn>`) with provenance pointing to the tier B ID and digest
- History file `blind-history/v1`: per set, rotation date and all published `blind-stats/v1` reports, append-only

**Out:**
- Generating the replacement set and sealing it (owned by `UNC.RDL.BLIND.CUST`)
- Freezing disclosed records into a release (owned by `UNC.RDL.FREEZE`)

## Requirements

- **REQ-UNC.RDL.BLIND.ROT-001** Disclosure shall convert a tier B set to tier G only if every regenerated record's digest equals the committed tier B digest, and otherwise fail with `blind_digest_mismatch` and convert nothing.
- **REQ-UNC.RDL.BLIND.ROT-002** Each disclosed record shall reference its former tier B ID and digest in its provenance.
- **REQ-UNC.RDL.BLIND.ROT-003** The history file shall only be appended to, and a CI check shall fail if an existing entry changes.

## Acceptance criteria

- **AC-1** Given the throwaway custody set of `UNC.RDL.BLIND.CUST` and its seeds, when disclosed, then tier G records are produced, each with the former tier B ID and digest.
- **AC-2** (error path) Given one wrong seed, then `blind_digest_mismatch` and no records.
- **AC-3** (error path) Given a commit that edits an existing history entry, then the CI check fails.

## Design

- **Registry command(s):** none (maintainer script under `tools/rdl/` using the built `fid rdl generate-dataset`, plus a library function in `fid-rdl`).
- **Data structures:** `blind-history/v1`.
- **Algorithm:** regenerate, compare digests, rewrite IDs and provenance, validate with the format.
- **Errors:** `blind_digest_mismatch`.
- **Determinism:** D0.
- **Performance budget:** trivial [unverified].
- **Crate:** `fid-rdl` (module `blind::rotate`) and a script.

## Test fixtures

Reuses `fixtures/rdl/blind/cust/` (CC-BY-4.0).

## Build prompt

> In crate `fid-rdl`, module `blind::rotate`, implement `disclose(tier_b_index, seeds)`: regenerate each case with the generator family and seed, require every digest to match the committed tier B digest (else `blind_digest_mismatch`, nothing converted), and emit tier G records `rdl:G/<aim>/blind-<set>/<nnnn>` whose provenance names the former tier B ID and digest. Add an append-only `blind-history/v1` file and a CI check that fails if an existing entry changes. Add a maintainer script under `tools/rdl/`. Tests AC-1..AC-3. CONSTITUTION 8.

## Open questions

- None.

## Children

_Leaf: none._
