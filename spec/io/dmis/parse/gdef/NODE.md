---
id: IO.DMIS.PARSE.GDEF
title: Grammar for header, unit, feature, tolerance and datum statements
level: 4
parent: IO.DMIS.PARSE
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.DMIS.PARSE-005]
covers: []
depends_on: [IO.DMIS.PARSE.TREE]
uses_artifacts: [IO.CORP.MANIF]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-20
---

# IO.DMIS.PARSE.GDEF: Grammar for definition and result statements

## Purpose

Registers descriptors and typed views for the statements that both DMIS output files and programs use: file header and bookkeeping, units, feature definitions and actuals, tolerance definitions and tolerance results, and datum definitions. This family is what the P1a output-file reader needs; program-only statements are in `IO.DMIS.PARSE.GEXEC`.

## Scope

**In:**
- Header and bookkeeping: `DMISMN`, `DMISMD`, `FILNAM`, `ENDFIL`, `TEXT`, `UNITS` [unverified: full list]
- Feature definitions `F(label)=FEAT/...` and actuals `FA(label)=FEAT/...` for point, line, plane, circle, arc, cylinder, cone, sphere, ellipse, slot, torus, pattern, compound, generic and edge-point types, with Cartesian/polar and inner/outer minor words [unverified: type list]
- Tolerance definitions `T(label)=TOL/...` and tolerance results `TA(label)=TOL/...` for size, location, form, orientation, profile and runout kinds, with `INTOL`/`OUTOL` flags on results [unverified: exact result syntax]
- Datum definitions: the unlabelled `DATDEF/<feature>,<datum label>` form and the labelled `DA(label)=DATDEF/<feature>` form of output files. The form `D(label)=DATDEF/...` is not supported: `D(..)` labels name coordinate systems (`IO.DMIS.PARSE.GEXEC`) [unverified]
- Typed views: `FeatView`, `TolView`, `DatumView`, `UnitsView` with values as lexemes
- QPId-carrying forms where DMIS 5.3 defines them, exposed on the views [unverified: which statements carry QPIds]

**Out:**
- Program-only statements (`IO.DMIS.PARSE.GEXEC`)
- Meaning of results (`IO.DMIS.OUT`) and definitions (`IO.DMIS.PROG`)

## Requirements

- **REQ-IO.DMIS.PARSE.GDEF-001** Each statement in this family shall parse into a `Statement` with a class from this family and a typed view exposing its label, feature or tolerance kind and its values as authored lexemes.
- **REQ-IO.DMIS.PARSE.GDEF-002** The parser shall accept the NIST DMIS Test Suite files, or parts of files, that use only statements of this family and lie inside the declared conformance classes [unverified: suite targets DMIS 5.2; licence of test files per header].
- **REQ-IO.DMIS.PARSE.GDEF-003** Each descriptor shall carry a support declaration (import `full` or `partial`, conformance module tags, proving test ID) for `IO.DMIS.CONF`.
- **REQ-IO.DMIS.PARSE.GDEF-004** QPIds on statements of this family shall be exposed unchanged on the typed views.

## Acceptance criteria

- **AC-1** Given `fixtures/io/dmis/parse/gdef/features.dmi` (one definition of each listed feature type, project-written), when parsed, then each yields its class and a `FeatView` with the expected kind and lexemes.
- **AC-2** Given `fixtures/io/dmis/parse/gdef/results.dmo` (feature actuals and tolerance results with `INTOL` and `OUTOL`), then each `TolView` exposes its result values and flag.
- **AC-3** Given the `nist-dmis` tagged entries of `corpus/dmis.toml` restricted to this family, then all parse without `Unknown` nodes for this family's keywords.
- **AC-4** Given the descriptors, then every one has a support declaration whose test ID exists.

## Design

- **Registry commands:** none (library).
- **Data structures:** descriptors in a static table; `FeatView { label, actual: bool, kind, coord: Cart|Pol, inout?, values: Vec<Lexeme>, qpid? }`; `TolView { label, actual: bool, kind, values, flag: Option<InOut>, refs: Vec<Label>, qpid? }`; `DatumView`; `UnitsView { length, angle, temp? }`.
- **Algorithm:** descriptor shapes written from the DMIS 5.3 syntax by the implementer (purchased copy, clean room; no text copied).
- **Errors:** via `IO.DMIS.PARSE.TREE`.
- **Determinism:** D0.
- **Performance budget:** as TREE.
- **Crate:** `fid-io-dmis`.

## Test fixtures

Project-written DMIS files under `fixtures/io/dmis/parse/gdef/` (CC-BY-4.0); `corpus/dmis.toml` entries tagged `nist-dmis` [unverified: licence].

## Build prompt

> In crate `fid-io-dmis`, register descriptors and typed views for header and bookkeeping statements, units, feature definitions and actuals (all listed feature types, Cartesian/polar, inner/outer), tolerance definitions and tolerance results (with INTOL/OUTOL), and datum definitions; values stay authored lexemes; QPIds exposed unchanged. Attach a support declaration (import level, conformance module tags, test ID) to every descriptor for the conformance generator. Write shapes from the DMIS 5.3 syntax clean-room (no text copied). CONSTITUTION §8: no panic, no unwrap. Tests AC-1..AC-4.

## Open questions

- Feature, tolerance and QPId statement lists to be confirmed against DMIS 5.3 [unverified].

## Children

_Leaf: none._
