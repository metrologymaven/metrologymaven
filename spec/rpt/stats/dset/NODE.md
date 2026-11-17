---
id: RPT.STATS.DSET
title: Data sets, ordering and exclusions
level: 3
parent: RPT.STATS
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.STATS-002, REQ-RPT.STATS-009]
covers: [PW.MSP.01]
depends_on: [RPT.CATALOG.QUERY, CORE.PRJ]
cli: [fid stats *-dataset]
mcp: [stats_*_dataset]
journal: [stats.*_dataset@1]
updated: 2026-11-17
---

# RPT.STATS.DSET: Data sets, ordering and exclusions

## Purpose

A data set is the exact, ordered input of every statistic: for each characteristic, the list of ResultRecords taken from the current project or from a catalog query, in a defined order, with every excluded record and its reason. Making it a first-class, digested object is what lets every statistic say which results it used (REQ-RPT.STATS-009) and be reproduced bit for bit.

## Scope

**In:**
- `DataSet` definition: source (project runs and stages, or a saved catalog query), characteristic list, ordering key (stage, run time, part sequence)
- Materialisation into ordered record lists with a data-set digest
- Automatic exclusions (`NOT_DECIDABLE`, voided readings, `STALE` catalog entries, non-matching units) and user exclusions with a reason
- Saving a data-set definition (W2) and computing one in scratch (R1)

**Out** (and which node owns it instead):
- Catalog storage and query execution (`RPT.CATALOG`)
- Any statistic over a data set (DESC, TREND, SPC, CAP, CMP)

## Requirements

- **REQ-RPT.STATS.DSET-001** A data set shall hold, per characteristic, an ordered list of ResultRecord IDs and the list of excluded record IDs each with a reason code.
- **REQ-RPT.STATS.DSET-002** The data-set order shall be total and deterministic: the chosen key (stage, run time or part sequence), then record ID.
- **REQ-RPT.STATS.DSET-003** When a data set is materialised, the system shall compute a digest over its definition, ordered record IDs, values and exclusions, and statistics shall record that digest.
- **REQ-RPT.STATS.DSET-004** Records with decision `NOT_DECIDABLE`, voided readings and catalog entries marked `STALE` shall be excluded automatically with those reason codes.
- **REQ-RPT.STATS.DSET-005** When the user excludes a record, the system shall require a reason text and journal the exclusion as a W2 command; the record itself shall not change.
- **REQ-RPT.STATS.DSET-006** Materialising the same definition against the same project journals and catalog state shall give the same digest.

## Interfaces

- **Offers:** `stats.compute_dataset@1` (R1), `stats.exclude_dataset@1` (W2), a save-definition command named at leaf level; types `DataSet`, `DataSetId`, `ExclusionReason`; a trait used by DESC, TREND, SPC, CAP and CMP.
- **Consumes:** catalog queries (`RPT.CATALOG.QUERY`), project runs and stages (`CORE.PRJ`).

## Decomposition hints

DEF (definition and save), MAT (materialisation, ordering and digest), EXCL (automatic and user exclusions).

## Open questions

- Resolved at leaf level: commands are `stats.save_dataset@1` (W2), `stats.list_dataset@1` (R0), `stats.compute_dataset@1` (R1), `stats.exclude_dataset@1` and `stats.reinclude_dataset@1` (W2). The piece ID that CMP needs is defined in DSET.MAT.
- Should user exclusions be stored on the data-set definition or as annotations on the records? Assumed on the definition, so the same record can be in or out in different analyses.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| RPT.STATS.DSET.DEF | Data-set definition and save (leaf) | 001 003 | |
| RPT.STATS.DSET.EXCL | Automatic and user exclusions (leaf) | 004 005 | |
| RPT.STATS.DSET.MAT | Data-set materialisation, ordering and digest (leaf) | 001 002 003 006 | PW.MSP.01 |
