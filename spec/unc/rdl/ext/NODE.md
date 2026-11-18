---
id: UNC.RDL.EXT
title: Tier X external dataset import
level: 3
parent: UNC.RDL
kind: module
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.RDL-001, REQ-UNC.RDL-002]
covers: []
depends_on: [UNC.RDL.FORMAT]
cli: [fid rdl import-external]
mcp: [rdl_import_external]
journal: [rdl.import_external@1]
updated: 2026-11-17
---

# UNC.RDL.EXT: Tier X external dataset import

## Purpose

Imports reference datasets published by national metrology institutes (the NIST Algorithm Testing System least-squares datasets, the PTB softgauge intercomparison set, and later surface softgauges) into `RdlDataset` records, preserving source digests, units and licences. Redistribution depends on licence terms that are not yet confirmed, so content stays `reference_only` until they are.

## Scope

**In:**
- One importer per source format, each bounded and fail-closed
- Unit handling: source mm values kept as decimals with the declared conversion to SI
- Licence and provenance capture; default visibility `reference_only` until a licence is confirmed
- `rdl.import_external@1` (W2, human-run; takes an import-root handle)

**Out:**
- The record format (owned by `UNC.RDL.FORMAT`)
- TraCIM order data (owned by `UNC.RDL.TRACIM`)

## Requirements

- **REQ-UNC.RDL.EXT-001** When an external dataset is imported, the system shall record its source URL or DOI, retrieval date, original file digest and licence, and produce one record per case.
- **REQ-UNC.RDL.EXT-002** Until a licence permitting redistribution is recorded with evidence, imported records shall have visibility `reference_only`.
- **REQ-UNC.RDL.EXT-003** Source values given in mm shall be stored as their original decimal strings with the conversion to SI declared, never as pre-multiplied f64.
- **REQ-UNC.RDL.EXT-004** If a source file is malformed or exceeds the importer's bounds, then the import shall fail with a typed error and store nothing.
- **REQ-UNC.RDL.EXT-005** The RDL shall import public filter reference data (for example NIST surface-metrology reference software outputs and type F1/F2 softgauges [unverified availability]) with licence evidence, so ISO 16610-family filter hypotheses have a public reference.
- **REQ-UNC.RDL.EXT-006** The RDL shall encode the published inputs of JCGM 100, 101 and 102 worked examples (designators and example numbers only, no copied text) as fixtures with a licence-evidence record, so uncertainty propagation can be checked against them.

## Interfaces

- **Offers:** tier X records; `rdl.import_external@1`.
- **Consumes:** `RdlDataset` format (UNC.RDL.FORMAT).

## Decomposition hints

NIST (ATS file format importer), PTBSG (PTB softgauge set importer), LIC (licence evidence records).

## Open questions

- Licence terms for redistributing NIST ATS data (US-government data, likely public domain) and the PTB softgauge set (reported CC-BY-4.0) are unverified [unverified]. Redistribution waits on confirmation, which is recorded through `UNC.RDL.EXT.LIC` evidence.
- The NIST ATS and PTB file formats have not been inspected for this spec [unverified], so `UNC.RDL.EXT.NIST` and `UNC.RDL.EXT.PTBSG` stay open. They can become leaves once a person has read the formats and written fixtures.
- `rdl.import_external@1` is `human_only: true` (it records provenance claims and retrieval facts that a person vouches for), so its MCP tool `rdl_import_external` is request-only (CONSTITUTION 3, D-031).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| UNC.RDL.EXT.IMP | Importer contract and the rdl.import_external@1 command (leaf) | 001 003 004 | none |
| UNC.RDL.EXT.LIC | Licence evidence records and visibility gating (leaf) | 002 | none |
| UNC.RDL.EXT.NIST | NIST Algorithm Testing System importer | 001 003 004 | none |
| UNC.RDL.EXT.PTBSG | PTB softgauge set importer | 001 003 004 | none |
| UNC.RDL.EXT.FILT | Filter softgauge importer | 005 | none |
| UNC.RDL.EXT.JCGM | JCGM worked-example fixtures | 006 | none |
