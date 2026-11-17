---
id: LVM.STATION.IMPORT
title: Import of recorded and file-based observations
level: 3
parent: LVM.STATION
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-LVM.STATION-004, REQ-LVM.STATION-005]
covers: []
depends_on: [LVM.STATION.OBS, HAL.XSCR, IO.REG]
cli: [fid net import-observations]
mcp: [net_import_observations]
journal: [net.import_observations@1]
updated: 2026-11-16
---

# LVM.STATION.IMPORT: Import of recorded and file-based observations

## Purpose

Brings observations into the network without a connected instrument: from HAL instrument I/O transcripts recorded earlier, and from observation files read by IO. This is what makes offline network adjustment (REQ-LVM-007) possible. Every imported record goes through the same validated constructor as a live observation, so all later commands behave identically.

## Scope

**In:**
- `net.import_observations@1` taking a transcript handle or an import-root file handle
- Mapping transcript or file records to stations and point names, including vendor angle conventions
- Atomic, all-or-nothing import with the failing record index
- Deterministic UUIDv5 observation IDs derived from the source digest and record index, so re-import is idempotent
- Size and record-count limits

**Out** (and which node owns it instead):
- Parsing file formats (`IO`, through a reader registered in `IO.REG`)
- Recording transcripts (`HAL.XSCR`)
- Single-record validation (`LVM.STATION.OBS`)

## Requirements

- **REQ-LVM.STATION.IMPORT-001** When `net.import_observations@1` is given a HAL transcript handle, the system shall create one observation per measurement record with provenance `transcript`, the device identity and the `simulated` bit taken from the transcript, and the instrument timestamps preserved.
- **REQ-LVM.STATION.IMPORT-002** When `net.import_observations@1` is given a file handle inside an approved import root, the system shall read it through the IO reader registered for its format and create observations with provenance `file`; agents shall pass only handles, never raw paths.
- **REQ-LVM.STATION.IMPORT-003** If any record fails validation or lacks a field required by its observation type, then the import shall store nothing, journal nothing and fail with `NET_IMPORT_INVALID` naming the zero-based record index and the field.
- **REQ-LVM.STATION.IMPORT-004** The system shall derive each imported observation ID as a UUIDv5 of the source digest and record index, so importing the same source twice into the same network is detected and reported as `NET_IMPORT_DUPLICATE` without creating new observations.
- **REQ-LVM.STATION.IMPORT-005** If a source exceeds the configured record limit (default 10 million records) [unverified] or the reader's byte limit, then the import shall fail closed with `NET_IMPORT_TOO_LARGE`.
- **REQ-LVM.STATION.IMPORT-006** Given the same observations captured live and imported from their transcript, the stored payloads shall be bit-identical.

## Interfaces

- **Offers:** `net.import_observations@1` (W2) with a dry-run mode that reports counts and the first error without storing.
- **Consumes:** `HAL.XSCR` transcript playback; `IO.REG` reader registry and import-root handles; `LVM.STATION.OBS` validated constructor; `LVM.STATION.MODEL` name resolution.

## Decomposition hints

XSCR (transcript mapping), FILE (file-reader mapping and vendor conventions), ATOMIC (all-or-nothing commit and duplicate detection).

## Open questions

- Which interchange format, if any, should carry raw network observations: a QIF extension through its sanctioned hooks, or a project-specific schema? Needs IO input; until then only HAL transcripts and plain tabular observation files are assumed.
- Which vendor observation exports should be supported first is a seed question for `seeds/devices.md`.

## Children

_None yet: status `open`._
