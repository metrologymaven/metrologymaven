---
id: UNC.RDL.GEN.XCHECK
title: Oracle cross-check recording and gating
level: 4
parent: UNC.RDL.GEN
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-rdl-gen]
built_pr: 501
satisfies: [REQ-UNC.RDL.GEN-003]
covers: []
depends_on: [UNC.RDL.FORMAT, UNC.RDL.GEN.HPREC]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.RDL.GEN.XCHECK: Oracle cross-check recording and gating

## Purpose

Merges an independent oracle's results into generated records. The oracle runs outside this repository (the `fiducial-oracles` repository, mpmath route) and emits an `oracle-result/v1` file with one entry per record digest. This node compares each oracle reference with the generator's reference within the declared test uncertainty, writes the cross-check entry (oracle ID, version, agreement, maximum deviation) into the record, and refuses to finalise a record that disagrees or has no oracle result.

## Scope

**In:**
- `oracle-result/v1` file schema: oracle ID, version, per record digest the reference outputs as decimal strings
- Comparison in `Hp` per output; agreement when the absolute difference is at most the record's test uncertainty
- Cross-check status per record: `agreed`, `disagreed`, `pending`
- Gating: `finalize(records, oracle_results)` fails with `oracle_disagreement` listing record IDs and changes no record

**Out:**
- Running the oracle (external repository)
- Freezing (owned by `UNC.RDL.FREEZE`, which should refuse records with status other than `agreed`)

## Requirements

- **REQ-UNC.RDL.GEN.XCHECK-001** For each record, the function shall record the oracle ID, version, status and maximum absolute deviation over outputs.
- **REQ-UNC.RDL.GEN.XCHECK-002** If any output's deviation exceeds the record's test uncertainty, then finalisation shall fail with `oracle_disagreement` listing every disagreeing record ID, and shall change no record.
- **REQ-UNC.RDL.GEN.XCHECK-003** If a record has no oracle entry, then its status shall be `pending`, and finalisation with `require_all = true` shall fail with `oracle_missing`.
- **REQ-UNC.RDL.GEN.XCHECK-004** If the oracle ID equals the generator's tool ID, then the function shall reject the file with `oracle_not_independent`.

## Acceptance criteria

- **AC-1** Given the LSNULL seed-1 records and `fixtures/rdl/gen/xcheck/oracle_ok.json`, when finalised, then every record has status `agreed` with its deviation.
- **AC-2** (error path) Given `oracle_one_bad.json` with one reference shifted by 1e-20 m and test uncertainty 1e-30 m, then `oracle_disagreement` naming that record, and records are unchanged.
- **AC-3** Given an oracle file missing two records and `require_all = false`, then those two are `pending`.
- **AC-4** (error path) Given an oracle file whose ID is `fid-rdl-gen`, then `oracle_not_independent`.

## Design

- **Registry command(s):** none; used by `UNC.RDL.GEN.CMD`.
- **Data structures:** `OracleResultFile`; `CrossCheck { oracle_id, oracle_version, status, max_dev_decimal }` (the format's `cross_check` entry).
- **Algorithm:** bounded parse (16 MiB); match by record digest; `Hp` subtraction.
- **Errors:** `oracle_disagreement{ids}`, `oracle_missing{ids}`, `oracle_not_independent`, `oracle_file_invalid`.
- **Determinism:** D0.
- **Performance budget:** 10 000 records in under 5 s [unverified until measured].
- **Crate:** `fid-rdl-gen` (module `xcheck`).

## Test fixtures

`fixtures/rdl/gen/xcheck/`: oracle files derived from the LSNULL seed-1 oracle output (CC-BY-4.0).

## Build prompt

> In crate `fid-rdl-gen`, module `xcheck`, define the `oracle-result/v1` schema (oracle ID, version, entries keyed by record digest with decimal-string references) and `finalize(records, oracle, require_all)`. For each record, compare outputs in `Hp` against the test uncertainty and write a cross-check entry (oracle ID, version, `agreed | disagreed | pending`, maximum deviation). Fail atomically with `oracle_disagreement` (all IDs) or `oracle_missing` (when `require_all`); reject an oracle whose ID equals the generator's (`oracle_not_independent`). Bounded parse (16 MiB). Tests AC-1..AC-4. CONSTITUTION 8.

## Open questions

- None.

## Children

_Leaf: none._
