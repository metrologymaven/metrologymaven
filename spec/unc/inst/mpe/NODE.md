---
id: UNC.INST.MPE
title: Specification-derived models from ISO 10360 MPE terms
level: 3
parent: UNC.INST
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.INST-002, REQ-UNC.INST-003]
covers: [SA.UNC.01]
depends_on: [UNC.INST.SCHEMA, UNC.MODEL.GRAPH]
cli: [fid unc derive-instrument-model]
mcp: [unc_derive_instrument_model]
journal: [unc.derive_instrument_model@1]
updated: 2026-11-17
---

# UNC.INST.MPE: Specification-derived models from ISO 10360 MPE terms

## Purpose

Builds UEC-1 instrument models from the maximum permissible errors a maker publishes under the ISO 10360 series (CMM length and probing errors, arm, tracker, optical sensor and optical 3D system terms). Each model states, term by term, which MPE term becomes which contributor and with what distribution assumption. An MPE is a limit for specific test tasks, not a distribution for an arbitrary task, so these models are always labelled specification-derived and UEC-1.

## Scope

**In:**
- One mapping table per ISO 10360 part in scope: term symbol, how its MPE formula (constant plus length-proportional part) becomes a contributor, the distribution assumption (for example rectangular over ±MPE) and the task applicability limits
- `unc.derive_instrument_model@1` (W2): from a maker's datasheet values to an `InstrumentModel` of kind `spec_derived`
- Citations by designator, edition and clause only

**Out:**
- The data format (owned by `UNC.INST.SCHEMA`)
- Profile curation (owned by `UNC.INST.LIB`)
- Per-observation covariance for portables (owned by `UNC.INST.PORT`)

## Requirements

- **REQ-UNC.INST.MPE-001** The system shall provide a term-to-contributor mapping for at least one CMM, one articulated-arm, one laser-tracker and one optical-scanner MPE term set [unverified: part-to-device mapping across ISO 10360-2, -5, -8, -10, -12, -13].
- **REQ-UNC.INST.MPE-002** Every derived model shall list, for each MPE term, the contributor it maps to, the distribution assumption and the clause cited.
- **REQ-UNC.INST.MPE-003** Every result whose uncertainty uses a specification-derived model shall be classed at most UEC-1.
- **REQ-UNC.INST.MPE-004** If a datasheet term required by a mapping is missing, then derivation shall fail with `term_missing` naming the term.
- **REQ-UNC.INST.MPE-005** No mapping table shall contain text copied from the standards; only symbols, designators and clause numbers.

## Interfaces

- **Offers:** mapping tables `mpe-map/<part>@<semver>`; command `unc.derive_instrument_model@1`.
- **Consumes:** instrument model format (UNC.INST.SCHEMA); graph contract (UNC.MODEL.GRAPH).

## Decomposition hints

One leaf per ISO 10360 part in scope (CMM2, ARM12, TRK10, OPT8, OPT13), plus DERIVE (the command and shared mapping engine).

## Open questions

- Split: the shared format, review state and derivation engine are agent-buildable (DERIVE); each part's table is content that must be checked against the purchased standard by an instrument-metrology expert, so those children stay open.

- Term symbols and their mapping must be checked against the purchased standards; ISO 10360-2 is under periodic review and ISO 10360-102 introduces a symbol grammar that may become the canonical identifier scheme [unverified]. Needs review by an instrument-metrology expert before leaves are finished.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| UNC.INST.MPE.DERIVE | Mapping-table format, review state and derivation command (leaf) | 002 003 004 005 | SA.UNC.01 |
| UNC.INST.MPE.CMM | CMM mapping table (ISO 10360-2 and -5) | 001 002 005 | SA.UNC.01 |
| UNC.INST.MPE.ARM | Articulated-arm mapping table (ISO 10360-12) | 001 002 005 | SA.UNC.01 |
| UNC.INST.MPE.TRK | Laser-tracker mapping table (ISO 10360-10) | 001 002 005 | SA.UNC.01 |
| UNC.INST.MPE.OPT | Optical-sensor and optical 3D system mapping tables (ISO 10360-8 and -13) | 001 002 005 | SA.UNC.01 |
