---
id: IO.RES.PIWEB
title: PiWeb-importable results export
level: 3
parent: IO.RES
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-IO.RES-011]
covers: [GOM.EXP.05]
depends_on: [IO.RES.AQDEF.OUT, IO.RES.AQDEF.KSET, IO.RES.TRANS.VIEW, IO.REG.ENV.EXPORT, IO.REG.WRITE]
side_effect: W1
determinism: D0
cli: [fid export results-piweb]
mcp: [export_results_piweb]
journal: [export.results_piweb@1]
updated: 2026-11-17
---

# IO.RES.PIWEB: PiWeb-importable results export

## Purpose

Exports results as a file that the PiWeb quality-data platform can import, so its users keep it downstream. No proprietary PiWeb format is reverse engineered: the export is a combined AQDEF DFQ file written by `IO.RES.AQDEF.OUT` with the `piweb` profile (the subset of publicly documented K-fields that PiWeb's documented DFQ import reads [unverified]) and a file name suited to PiWeb's folder-based automatic import [unverified]. If that public basis cannot be confirmed, the command is not registered and the export is left to a format plugin.

## Scope

**In:**
- `export.results_piweb@1` (W1, `fs_effect: write_root`, CONSTITUTION §3.1)
- The `piweb` profile in `aqdef-kset@1` with a public citation for each field and for PiWeb's DFQ import support
- File naming: `<part>_<run or first stage>_<UTC date of the newest result>.dfq`, sanitised to `[A-Za-z0-9._-]`
- Record status and source software for transcribed records in the profile's free-text field
- A registration gate: the command registers only when every `piweb` profile row and the PiWeb import citation are present

**Out:**
- Uploading to a PiWeb server or its web API (X1 egress through `egress.send@1`; not offered by IO)
- PiWeb's own native formats (plugin route only, `IO.REG.PLUGIN`)
- DFQ writing itself (`IO.RES.AQDEF.OUT`)

## Requirements

- **REQ-IO.RES.PIWEB-001** The system shall export ResultRecords for PiWeb as a combined DFQ file written by `IO.RES.AQDEF.OUT` with the `piweb` profile, built only from publicly documented format information [unverified: PiWeb's documented DFQ import].
- **REQ-IO.RES.PIWEB-002** If the `piweb` profile lacks a public citation for any field or for PiWeb's DFQ import support, then `export.results_piweb@1` shall not be registered, `export.list_formats@1` shall not list it, and the documentation shall point to the plugin route.
- **REQ-IO.RES.PIWEB-003** Exported values of transcribed records shall carry record status `transcribed` and source software, and the export shall pass the `IO.RES.TRANS.VIEW` conformance kit.
- **REQ-IO.RES.PIWEB-004** The file name shall be derived only from part identity, run or stage and result date, with every character outside `[A-Za-z0-9._-]` replaced by `_`, inside a target directory handle given by the caller.
- **REQ-IO.RES.PIWEB-005** The export report shall state `not Q-DAS certified` and `PiWeb import not verified by the vendor`.

## Acceptance criteria

- **AC-1** Given `fixtures/io/res/piweb/records.json` (part `P/1`, 3 characteristics × 4 stages, one transcribed) and export root `out`, when `fid export results-piweb --stages S1..S4 --target-dir root://out/ --json` runs, then one file `P_1_<run>_<date>.dfq` exists, equals `piweb.golden.dfq`, and contains only `piweb` profile K-fields.
- **AC-2** Given AC-1's file, then the transcribed value's free-text field holds `transcribed` and its source software, and the conformance kit passes.
- **AC-3** Given AC-1's file read back with `IO.RES.AQDEF.IN`, then all value texts equal the input.
- **AC-4** Given a test build where one `piweb` profile row has no citation, then `fid export list-formats --json` does not list `results-piweb` and invoking the command fails `command_not_found`.
- **AC-5** (error path) Given an existing file of the same name and no `--overwrite`, then `target_exists`.
- **AC-6** (replay) Given AC-1's journal replayed into an empty root, then `IDENTICAL`.

## Design

- **Registry command:** `export.results_piweb@1` W1 `fs_effect: write_root`; params `run?: RunId`, `stages?: [StageId]`, `characteristics?: [CharRef]`, `target_dir: RootHandle`, `overwrite: bool = false`, `dry_run: bool = false`; result `ExportReport` with the file name.
- **Data structures:** none new; `piweb` profile rows in `aqdef-kset@1`.
- **Algorithm:** check the registration gate at start-up → compute the file name → call the AQDEF writer with profile `piweb` into the atomic writer → append report statements.
- **Errors:** `no_results`, `kfield_value_too_long`, envelope errors.
- **Determinism:** D0 (the date in the file name comes from the newest result's recorded time, not the clock).
- **Performance budget:** as `IO.RES.AQDEF.OUT`.
- **Crate:** `fid-io-res` (module `piweb`).

## Test fixtures

`fixtures/io/res/piweb/` records and golden DFQ (CC-BY-4.0). A manual acceptance run on a real PiWeb installation is recorded separately when a user offers one [unverified].

## Build prompt

> In crate `fid-io-res`, add module `piweb` registering `export.results_piweb@1` (W1, `fs_effect: write_root`; CLI `fid export results-piweb`, MCP `export_results_piweb`) only when the `piweb` profile of `aqdef-kset@1` has a public citation for every field and for PiWeb's DFQ import support (otherwise do not register it and document the plugin route). The command computes a file name from part identity, run or stage and the newest result's recorded date (characters outside `[A-Za-z0-9._-]` become `_`) inside the caller's target directory handle, writes a combined DFQ via `aqdef::out` with profile `piweb`, marks transcribed records via `trans::view` (run its conformance kit), and states `not Q-DAS certified` and `PiWeb import not verified by the vendor` in the report. No upload, no network. CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-6.

## Open questions

- Public documentation of PiWeb's DFQ import (and its auto-importer folder conventions) is [unverified]. If none can be cited, REQ-002 keeps the core honest by not registering the command; dropping GOM.EXP.05 from first-party scope entirely would then need a maintainer decision.
- A human acceptance test on a PiWeb installation is needed before claiming parity in `ECO.PARITY` [unverified].
- Finished as a leaf on the "research spike first" hint: the spike is reduced to filling the `piweb` profile citations, and the gate in REQ-002 makes the outcome safe either way.

## Children

_Leaf: none._
