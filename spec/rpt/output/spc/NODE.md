---
id: RPT.OUTPUT.SPC
title: Statistics export to SPC systems
level: 3
parent: RPT.OUTPUT
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.OUTPUT-006]
covers: [PCD.RPT.05, PCD.RPT.06]
depends_on: [RPT.OUTPUT.META, RPT.STATS.DSET, IO.RES.AQDEF, IO.RES.QIFR, IO.RES.CSV, CORE.STO.CAS]
side_effect: [W1, W1, W1, W1]
fs_effect: write_root
determinism: D0
cli: [fid stats export-qdas, fid stats export-qif, fid stats export-csv, fid stats stage-export-spc]
mcp: [stats_export_qdas, stats_export_qif, stats_export_csv, stats_stage_export_spc]
journal: [stats.export_qdas@1, stats.export_qif@1, stats.export_csv@1, stats.stage_export_spc@1]
updated: 2026-11-17
---

# RPT.OUTPUT.SPC: Statistics export to SPC systems

## Purpose

Sends the per-characteristic results of a data set to SPC systems as files: Q-DAS AQDEF (not certified), QIF Statistics and CSV. RPT decides which records go out, in which order and with which exclusion context; the byte-level writers stay in `IO.RES`. Writing into a local export root is W1. Delivery to a network SPC destination is never done here: `stats.stage_export_spc@1` stages the same bytes in the content-addressed store, and the user sends them with the CORE primitive `egress.send@1` (X1, request-only for agents), as CONSTITUTION §3.1 requires.

## Scope

**In:** mapping a `DataSet` (records per characteristic in data-set order, exclusions, characteristic metadata, transcription status) to the `IO.RES` writer inputs; `stats.export_qdas@1`, `stats.export_qif@1`, `stats.export_csv@1` (W1, `write_root`); `stats.stage_export_spc@1` (W1) staging an export for egress; the "not Q-DAS certified" label; export metadata with per-characteristic exclusion counts.

**Out:** AQDEF, QIF and CSV byte writers and column maps (`IO.RES.AQDEF`, `IO.RES.QIFR`, `IO.RES.CSV`); data-set selection and exclusions (`RPT.STATS.DSET`); export-root writes and `ExportMeta` (`RPT.OUTPUT.META`); network delivery and destination configuration (`CORE.BUS.POL.EGRESS`, `egress.send@1`).

## Requirements

- **REQ-RPT.OUTPUT.SPC-001** When an export command runs for a data set, the system shall pass the data set's non-excluded records per characteristic, in data-set order, to the `IO.RES` writer for that format, and write the result through the export sink.
- **REQ-RPT.OUTPUT.SPC-002** Records excluded from the data set shall not be exported, and the `ExportMeta` extension and the command result shall list the exclusion count per characteristic, sorted by characteristic ID.
- **REQ-RPT.OUTPUT.SPC-003** Every AQDEF export shall state that it is not Q-DAS certified in its metadata and in the command result.
- **REQ-RPT.OUTPUT.SPC-004** The three export commands shall be side-effect class W1 with `fs_effect: write_root` and journal one event each with the data-set digest and the sink receipt.
- **REQ-RPT.OUTPUT.SPC-005** When `stats.stage_export_spc@1` runs, the system shall build the same bytes as the matching export command, store them in the content-addressed store as a staged egress artifact, journal one W1 event with the digest, and return the digest for use with `egress.send@1`; it shall open no network connection.
- **REQ-RPT.OUTPUT.SPC-006** Records with status `transcribed` shall keep that status and their source software name and version in every format that has a field for them, and the CSV export shall add `record_status` and `source_software` columns.
- **REQ-RPT.OUTPUT.SPC-007** Exporting the same data set twice with the same writer versions shall give byte-identical files.
- **REQ-RPT.OUTPUT.SPC-008** If the data set is empty after exclusions, then the command shall fail with error `stats.dataset_empty` and write nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/stats/dset_25parts` (5 characteristics, 25 parts, 2 exclusions), when `fid stats export-qif --dataset <id> --root <h> --name lot7.qif --json` runs, then the file validates against the QIF 3.0 Statistics schema [unverified schema availability], holds 123 records in data-set order, and the result lists exclusion counts per characteristic.
- **AC-2** Given AC-1's data set, when `fid stats export-qdas` runs, then the DFQ file contains only documented K-fields as written by `IO.RES.AQDEF`, and both metadata and result say "not Q-DAS certified".
- **AC-3** Given a data set with transcribed records, when `fid stats export-csv` runs, then the `record_status` column shows `transcribed` and `source_software` names the source and version.
- **AC-4** Given AC-1, when `fid stats stage-export-spc --format qif --dataset <id> --json` runs under a socket-denying sandbox, then it succeeds, returns a digest equal to the SHA-256 of AC-1's file, and one W1 event is journaled.
- **AC-5** (error path) Given a data set with every record excluded, then each command fails with `stats.dataset_empty` and writes nothing.
- **AC-6** (determinism) Given AC-1 run twice with overwrite, then the files are byte-identical.

## Design

- **Registry commands:**
  - `stats.export_qdas@1 | stats.export_qif@1 | stats.export_csv@1(dataset: DataSetId, root: ExportRootHandle, name: ExportName, overwrite: bool, dry_run: bool) -> SpcExportResult` (W1, `write_root`)
  - `stats.stage_export_spc@1(dataset: DataSetId, format: Qdas | Qif | Csv) -> StagedArtifact { digest, bytes }` (W1)
- **Data structures:** `SpcExportResult { receipt, exclusions: Vec<(CharacteristicId, u32)>, certified: false (AQDEF only) }`; the exclusion list goes into `ExportMeta` as an extension field.
- **Algorithm:** load the data set -> filter exclusions -> map to the writer input type of the format -> call the `IO.RES` writer to bytes -> sink or staging store.
- **Errors:** `stats.dataset_empty`, `report.not_found`, writer errors, sink errors.
- **Determinism:** D0.
- **Performance budget:** export 100 characteristics by 1 000 parts in under 2 s [unverified until measured].
- **Crate:** `fid-report-out` (module `spc`).

## Test fixtures

`fixtures/rpt/stats/dset_25parts` (synthetic data set from `RPT.STATS.DSET` tests, with two transcribed records); CC-BY-4.0.

## Build prompt

> In crate `fid-report-out`, add module `spc` with commands `stats.export_qdas@1`, `stats.export_qif@1`, `stats.export_csv@1` (W1, `fs_effect: write_root`; CLI `fid stats export-qdas | export-qif | export-csv --json`, dry run) and `stats.stage_export_spc@1` (W1; CLI `fid stats stage-export-spc`). Map the data set's non-excluded records per characteristic, in data-set order, to the `IO.RES` writer inputs; list exclusion counts per characteristic in the result and in `ExportMeta`; label AQDEF output "not Q-DAS certified"; keep `transcribed` status and source software (CSV adds `record_status`, `source_software`); empty data set -> `stats.dataset_empty`. File exports go through the export sink; staging stores the same bytes in the content-addressed store and returns the digest for `egress.send@1`. Never open a network connection. Acceptance: AC-1..AC-6 in `spec/rpt/output/spc/NODE.md`. Constraints: CONSTITUTION §3.1 (local export W1; data leaving the machine only via `egress.send@1`), C7, C8, §8.

## Open questions

- Which network SPC destinations to support is no longer RPT's question: destinations are administrator-configured `egress.send@1` targets (network share or HTTPS endpoint). Vendor database APIs, if ever wanted, would be plugins.
- The class W1 for staging an egress artifact assumes staging is a draft-level effect, since nothing leaves the machine and no working document changes; confirm with `CORE.BUS.POL.EGRESS`, which leaves staging to each domain.

## Children

_Leaf: none._
