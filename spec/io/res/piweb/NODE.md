---
id: IO.RES.PIWEB
title: PiWeb-importable results export
level: 3
parent: IO.RES
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-IO.RES-011]
covers: [GOM.EXP.05]
depends_on: [IO.RES.TRANS, IO.REG.WRITE]
cli: [fid export results-piweb]
mcp: [export_results_piweb]
journal: [export.results_piweb@1]
updated: 2026-11-16
---

# IO.RES.PIWEB: PiWeb-importable results export

## Purpose

Exports results in a form that PiWeb can import, so users of that quality-data platform can keep using it downstream. Whether a public format specification exists decides whether this is first-party or plugin-only.

## Scope

**In:**
- A file-based export PiWeb can import (format to be fixed from public documentation) [unverified]
- `export.results_piweb@1` (W1, `fs_effect: write_root`)

**Out:**
- Uploading to a PiWeb server (X1 egress; not offered by IO)

## Requirements

- **REQ-IO.RES.PIWEB-001** The system shall export ResultRecords to a file that PiWeb imports without manual mapping, built only from publicly documented format information [unverified: availability].
- **REQ-IO.RES.PIWEB-002** If no public format specification is available, then this export shall be offered only through a format plugin (`IO.REG.PLUGIN`) and the core shall ship none.
- **REQ-IO.RES.PIWEB-003** Exported values shall carry record status and source software for transcribed records.

## Interfaces

- **Offers:** `export.results_piweb@1` (W1 write_root) or a plugin descriptor.
- **Consumes:** `IO.RES.TRANS`, `IO.REG.WRITE`.

## Decomposition hints

Research spike first (public format documentation and licence), then WRITE.

## Open questions

- Public specification and licence for PiWeb's import formats are [unverified]. If none, this node shrinks to a plugin note; if that would drop GOM.EXP.05 from first-party scope, a decision is needed.

## Children

_None yet: status `open`._
