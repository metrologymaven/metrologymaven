---
id: IO.DMIS.OUT
title: DMIS output-file reader for measured results
level: 3
parent: IO.DMIS
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.DMIS-003, REQ-IO.DMIS-006]
covers: []
depends_on: [IO.DMIS.PARSE.GDEF, IO.DMIS.PARSE.TREE]
uses_artifacts: [IO.CORP.MANIF]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.DMIS.OUT: DMIS output-file reader

## Purpose

Turns a parsed DMIS output file (`.dmo`) into a typed list of reported feature actuals and tolerance results, exactly as written, plus the header facts the transcription importer needs (producing software if stated, units in force). It interprets nothing: values stay authored lexemes with the unit text in force at that statement, and status flags are passed through. `IO.RES.TRANS.DMIS` turns the rows into transcription records.

## Scope

**In:**
- Walking the `DmisTree` in order with a unit state (length and angle units from each `UNITS` statement)
- Feature actuals (`FA(label)=FEAT/...`): label, feature kind, actual values as lexemes, unit text
- Tolerance results (`TA(label)=TOL/...`): label, tolerance kind, nominal and limits where written (from the matching `T(label)` definition in the same file if present), actual, deviation, `INTOL`/`OUTOL` flag or none, feature labels referenced, unit text
- Header facts: values of `DMISMN`, `DMISMD` and `FILNAM` as untrusted text [unverified: which of these name the producing software and version]
- QPIds present on output statements carried onto rows unchanged
- Read report: every statement not mapped (excluding pure bookkeeping), with line and statement class or `Unknown` keyword

**Out:**
- The `import.results_dmis@1` command and transcription records (`IO.RES.TRANS.DMIS`)
- Parsing (`IO.DMIS.PARSE`), program semantics (`IO.DMIS.PROG`)

## Requirements

- **REQ-IO.DMIS.OUT-001** When a DMIS output file is read, each reported feature actual and tolerance result shall be extracted with its label, nominal, actual, deviation, unit and out-of-tolerance flag as written, with absent fields left empty rather than computed.
- **REQ-IO.DMIS.OUT-002** Numeric values shall keep their authored lexeme and the length or angle unit text in force at their statement.
- **REQ-IO.DMIS.OUT-003** QPIds present on output statements shall be carried onto the extracted rows unchanged.
- **REQ-IO.DMIS.OUT-004** Output statements the reader does not map shall be listed with line number and statement class (or `Unknown` keyword) in the read report.
- **REQ-IO.DMIS.OUT-005** Each tolerance row shall state whether a feature actual for each referenced feature label appears in the same file.
- **REQ-IO.DMIS.OUT-006** Header statement values shall be returned as untrusted text, and the reader shall not infer a software name from any other content.

## Acceptance criteria

- **AC-1** Given `fixtures/io/dmis/out/twelve_results.dmo` (project-written: header, units mm, 6 feature actuals, 12 tolerance results, 2 `OUTOL`), when read, then 6 feature rows and 12 tolerance rows are returned, 2 flagged out of tolerance, all values as lexemes with unit `MM`.
- **AC-2** Given `fixtures/io/dmis/out/units_switch.dmo` (units change from mm to inch midway), then rows after the switch carry unit `INCH` and values unchanged.
- **AC-3** Given `fixtures/io/dmis/out/qpids.dmo`, then each row carries its QPId exactly.
- **AC-4** Given `fixtures/io/dmis/out/with_unknown.dmo` (one vendor statement), then the read report lists it with its line and keyword and the other rows are returned.
- **AC-5** Given a tolerance result whose feature actual is absent, then `feature_actual_present: false` for that label.
- **AC-6** Given every `accept` output-file entry of `corpus/dmis.toml`, then row counts match the manifest.

## Design

- **Registry commands:** none (library; the command is `import.results_dmis@1` in `IO.RES.TRANS.DMIS`).
- **Data structures:** `DmisOutput { header: HeaderFacts { dmismn?, dmismd?, filnam?: Untrusted }, features: Vec<FeatRow>, tolerances: Vec<TolRow>, report: ReadReport }`; `FeatRow { label, kind, values: Vec<Lexeme>, unit: UnitText, qpid?, line }`; `TolRow { label, kind, nominal?, upper?, lower?, actual?, deviation?: Lexeme, flag: Option<InTol|OutTol>, features: Vec<(Label, bool)>, unit, qpid?, line }`.
- **Algorithm:** single pass; definitions `T(label)` remembered in a `BTreeMap` to supply nominal and limits to later `TA(label)`; unit state updated on `UNITS`.
- **Errors:** none beyond parse errors; unmapped items are report entries.
- **Determinism:** D0.
- **Performance budget:** 10^5 rows in < 1 s [unverified until measured].
- **Crate:** `fid-io-dmis`.

## Test fixtures

Project-written output files under `fixtures/io/dmis/out/` (CC-BY-4.0); output-file entries in `corpus/dmis.toml` (`IO.CORP.MANIF`), including files exported from incumbents by volunteers with permission [unverified].

## Build prompt

> In crate `fid-io-dmis`, implement `read_dmis_output(&DmisTree) -> DmisOutput`: walk statements in order with a unit state; extract feature actuals (label, kind, lexeme values, unit text, QPId, line) and tolerance results (label, kind, nominal/limits from a same-file `T(label)` definition if present, actual, deviation, INTOL/OUTOL flag or none, referenced feature labels with whether their actuals appear in the file, unit text, QPId, line), never computing missing fields; return header statement values as untrusted text; list unmapped statements with line and class or keyword. CONSTITUTION §8: no panic, no unwrap; BTreeMap only. Tests AC-1..AC-6.

## Open questions

- Which header statements carry the producing software's name and version [unverified; shared with `IO.RES.TRANS.DMIS`].
- Resolved in level-3 review: the single command is `import.results_dmis@1` in `IO.RES.TRANS.DMIS`; this leaf is a library.

## Children

_Leaf: none._
