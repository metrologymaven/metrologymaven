---
id: IO.CAD.IGES.SECT
title: Bounded IGES fixed-column section parser and writer
level: 4
parent: IO.CAD.IGES
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.CAD.IGES-003, REQ-IO.CAD.IGES-005]
covers: []
depends_on: [IO.REG.LIMITS, IO.REG.PROV]
uses_artifacts: [IO.CORP.MANIF]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.CAD.IGES.SECT: Bounded IGES fixed-column section parser and writer

## Purpose

The encoding layer for IGES files: a bounded parser for the 80-column ASCII form (Start, Global, Directory Entry, Parameter Data and Terminate sections) that yields the Global parameters, a directory of entries and each entry's parameter list, and a writer that produces the same form for `IO.CAD.EXPORT.IGESW`. It checks every cross-reference between sections before any entity is interpreted.

## Scope

**In:**
- Line classification by the section letter in column 73 and sequence numbers in columns 74–80, with sequence continuity checks
- Global section: parameter and record delimiters, Hollerith strings, model-space scale, unit flag, unit name, minimum resolution, maximum coordinate, originating system (untrusted)
- Directory entries: two lines of ten 8-column fields each (entity type, parameter pointer, structure, line font, level, view, transform pointer, label display, status, form, label, subscript, ...)
- Parameter Data: free-format records joined across lines up to the record delimiter, back-pointer to the DE checked
- Terminate section counts checked against actual section lengths
- Writer: deterministic fixed-column output with Hollerith strings and sequence numbers
- Unit resolution from the unit flag (and unit name when the flag says so) to an exact factor to metres

**Out:**
- Entity semantics (`IO.CAD.IGES.ENT`)
- Compressed ASCII and binary IGES forms: rejected with `iges_form_unsupported` [unverified: frequency in practice]

## Requirements

- **REQ-IO.CAD.IGES.SECT-001** If a directory entry's parameter pointer or line count points outside the Parameter Data section, then parsing shall fail with `malformed` naming the DE sequence number.
- **REQ-IO.CAD.IGES.SECT-002** If a parameter record's back-pointer does not equal its DE's sequence number, then parsing shall fail with `malformed` naming both numbers.
- **REQ-IO.CAD.IGES.SECT-003** The parser shall resolve the Global unit flag (and unit name where the flag requires it) to an exact factor to metres, and if neither resolves, then it shall return `unit_required` unless the caller supplied `unit`.
- **REQ-IO.CAD.IGES.SECT-004** If the Terminate section counts disagree with the section line counts, then parsing shall fail with `truncated_file` naming the section.
- **REQ-IO.CAD.IGES.SECT-005** Real values shall be parsed with correct rounding (including the `D` exponent form) and keep their authored lexeme; Hollerith strings shall be returned as untrusted text.
- **REQ-IO.CAD.IGES.SECT-006** For every accepted file in the IGES corpus, parse → write → parse shall yield identical Global parameters, directory and parameter lists.

## Acceptance criteria

- **AC-1** Given `fixtures/io/cad/iges/sect/three_entities.igs` (a point, a line, an arc; unit flag mm), when parsed, then 3 DEs with their types, labels and parameter lists exist and the unit factor is 0.001.
- **AC-2** (error path) Given `fixtures/io/cad/iges/sect/bad_pointer.igs`, then parsing fails `malformed{de: 3}`.
- **AC-3** (error path) Given `fixtures/io/cad/iges/sect/bad_terminate.igs`, then `truncated_file{section: P}`.
- **AC-4** Given `fixtures/io/cad/iges/sect/d_exponent.igs` (`1.5D-3`), then the value is 0.0015 with lexeme `1.5D-3`.
- **AC-5** Given every `accept` entry of `corpus/iges.toml` (`IO.CORP.MANIF`), then parse → write → parse is identical.
- **AC-6** Given the fuzz target run for its CI budget, then no panic occurs.

## Design

- **Registry commands:** none (library).
- **Data structures:** `IgesFile { start: Vec<Untrusted>, global: GlobalParams, directory: BTreeMap<u32, DirEntry>, params: BTreeMap<u32, Vec<IgesParam>> }`; `IgesParam` (`Int`, `Real(f64, Lexeme)`, `Str(Untrusted)`, `Ptr(i32)`, `Default`); `UnitResolution { factor_m, source }`.
- **Algorithm:** stream lines (80 columns + optional line ending; longer lines → `malformed`), classify, check sequence continuity, parse Global with its declared delimiters, parse DEs, then parameter records by pointer with back-pointer check; bounds charged to `Budget`.
- **Errors:** `malformed{de?, line}`, `truncated_file{section}`, `unit_required`, `iges_form_unsupported`, `limit_exceeded`.
- **Determinism:** D0.
- **Performance budget:** ≥ 50 MB/s on reference hardware [unverified until measured].
- **Crate:** `fid-io-iges`.

## Test fixtures

Hand-written IGES files under `fixtures/io/cad/iges/sect/` (CC-BY-4.0); `corpus/iges.toml` (`IO.CORP.MANIF`).

## Build prompt

> In crate `fid-io-iges`, implement a bounded parser and writer for fixed-column ASCII IGES: classify 80-column lines by section letter (S, G, D, P, T) with sequence checks; parse Global parameters with their declared delimiters and Hollerith strings (untrusted text); parse two-line directory entries; join parameter records by pointer and check back-pointers (`malformed{de}`); check Terminate counts (`truncated_file{section}`); resolve units from the unit flag/name to exact factors (else `unit_required` unless caller `unit`); reals correctly rounded incl. `D` exponents, keep lexemes. Writer emits deterministic fixed-column output. Reject compressed/binary forms (`iges_form_unsupported`). Use `fid-io-reg` `Budget`. CONSTITUTION §8: no panic, no unwrap; BTreeMap only. Fuzz target. Tests AC-1..AC-6.

## Open questions

- IGES edition: 5.3 assumed [unverified; carried from IO.CAD.IGES].

## Children

_Leaf: none._
