---
id: RPT.STATS.DSET
title: Data sets, ordering and exclusions
level: 3
parent: RPT.STATS
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.STATS-002, REQ-RPT.STATS-009]
covers: [PW.MSP.01]
depends_on: [RPT.CATALOG.QUERY, CORE.PRJ]
cli: [fid stats compute-dataset, fid stats exclude-dataset]
mcp: [stats_compute_dataset, stats_exclude_dataset]
journal: [stats.compute_dataset@1, stats.exclude_dataset@1]
updated: 2026-11-16
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

- Should user exclusions be stored on the data-set definition or as annotations on the records? Assumed on the definition, so the same record can be in or out in different analyses.

## Children

_None yet: status `open`._
