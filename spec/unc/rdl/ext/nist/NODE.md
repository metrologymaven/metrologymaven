---
id: UNC.RDL.EXT.NIST
title: NIST Algorithm Testing System importer
level: 4
parent: UNC.RDL.EXT
kind: task
status: open
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.RDL.EXT-001, REQ-UNC.RDL.EXT-003, REQ-UNC.RDL.EXT-004]
covers: []
depends_on: [UNC.RDL.EXT.IMP]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.RDL.EXT.NIST: NIST Algorithm Testing System importer

## Purpose

An `ExternalImporter` for the NIST Algorithm Testing System (ATS) least-squares datasets: it parses the published files into one external case per dataset, with input points and reference fit results kept as their original decimal strings and units. It stays open because the file format has not been inspected for this spec.

## Scope

**In:**
- Format card written from the publisher's public documentation, in our own words
- Bounded, fail-closed parser for the published files
- Mapping of each dataset's aim to the kernel aim (`fit.<primitive>.ls`)

**Out:**
- The importer contract and command (owned by `UNC.RDL.EXT.IMP`)
- Licence evidence (owned by `UNC.RDL.EXT.LIC`)

## Requirements

- **REQ-UNC.RDL.EXT.NIST-001** The importer shall produce one external case per published dataset with its points, reference outputs and the source file digest.
- **REQ-UNC.RDL.EXT.NIST-002** The importer shall keep every numeric value as its original decimal text with its unit.
- **REQ-UNC.RDL.EXT.NIST-003** If a file deviates from the format card or exceeds the declared bounds, then the importer shall fail with `import_malformed` or `limit_exceeded`.

## Interfaces

- **Offers:** importer registered for `rdl.import_external@1`.
- **Consumes:** `ExternalImporter` trait (UNC.RDL.EXT.IMP).

## Decomposition hints

Likely a single leaf once a person has read the format and committed reference-only fixture digests.

## Open questions

- File format and licence unverified [unverified]; a person must inspect the files before this node is finished.

## Children

_None yet: status `open`._
