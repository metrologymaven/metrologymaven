---
id: UNC.UEC.QIF
title: QIF 3.0 mapping of the uncertainty statement
level: 3
parent: UNC.UEC
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-kunc]
built_pr: 492
satisfies: [REQ-UNC.UEC-009]
covers: []
depends_on: [UNC.UEC.STMT, UNC.UEC.CLASS]
uses_artifacts: [CORE.PRJ.QIF, IO.QIF.GEN.PIN]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.UEC.QIF: QIF 3.0 mapping of the uncertainty statement

## Purpose

Maps an `UncertaintyStatement` to and from QIF 3.0 results so that class, method, coverage, model digest and sources survive export and import. The full statement always travels as one versioned extension element, `fx:UncertaintyStatement` (`unc-qif-stmt@1`), placed in QIF's generic user-data extension hook. The numeric values that QIF can hold natively are written there as well, for readers that ignore extensions. The extension is authoritative on import. The mapping is pure (`fid-kunc`, L0) and works on a neutral fragment model, so the IO writers and readers (L2) can call it without UNC depending on IO.

## Scope

**In:**
- Mapping table `qif-unc-map@1`: statement field, QIF native element (where one exists) or extension element, ISO 23952 clause number only
- `to_qif(statement) -> UncQifFragment { native: NativeUncFields, ext_xml: Vec<u8> }` and `from_qif(fragment) -> UncertaintyStatement`
- The `unc-qif-stmt@1` element schema (XSD fragment), published for inclusion in the CORE extension schema
- UEC-0 export: an extension element with class UEC-0 and the reason, and no native numeric element
- Import policy: extension present means it is authoritative and native values must agree; native only means UEC-1 `DECLARED` with source `qif_import`

**Out:**
- QIF XML reading, writing and XSD validation (owned by `IO.QIF` and `IO.RES.QIFR`, which call this mapping)
- The extension catalogue and namespace (owned by `CORE.PRJ.QIF`, read as an artifact)
- The statement type (owned by `UNC.UEC.STMT`)

## Requirements

- **REQ-UNC.UEC.QIF-001** When a statement of any class is mapped with `to_qif` and back with `from_qif`, the resulting statement's canonical digest shall equal the original's.
- **REQ-UNC.UEC.QIF-002** When a UEC-0 statement is mapped, the fragment shall contain the not-evaluated marker with its reason and no native numeric uncertainty field.
- **REQ-UNC.UEC.QIF-003** The extension element shall use only the generic user-data extension hook of QIF 3.0, and fixture documents carrying it shall validate against the pinned QIF 3.0 XSD plus the CORE extension schema with no schema changes.
- **REQ-UNC.UEC.QIF-004** If a fragment carries native uncertainty values but no extension element, then `from_qif` shall return a UEC-1 statement with method `DECLARED` and source kind `Declared` with id `qif_import`, never UEC-2 or UEC-3.
- **REQ-UNC.UEC.QIF-005** If both are present and a native value differs from the extension value by more than 1 ulp, then `from_qif` shall fail with `qif_unc_conflict` naming the field.
- **REQ-UNC.UEC.QIF-006** If the extension element names an unknown version or exceeds 1 MiB, then `from_qif` shall fail with `unsupported_extension` or `limit_exceeded`.

## Acceptance criteria

- **AC-1** Given `fixtures/unc/stmt/valid/*.json` (one statement per class and method), when each is mapped to a fragment and back, then the digests equal the originals.
- **AC-2** Given the UEC-0 fixture, then the fragment has no native numeric field and the extension carries `UEC0` and its reason.
- **AC-3** Given `fixtures/unc/qif/docs/*.qif` (one QIF Results document per class, with fragments produced by this mapping), when validated by the pinned XSD and the CORE extension schema in CI, then all are valid.
- **AC-4** Given `fixtures/unc/qif/native_only.qif` fragment, then the statement is UEC-1, method `DECLARED`, source `qif_import`.
- **AC-5** (error path) Given `fixtures/unc/qif/conflict.qif` (native U differs from the extension U), then `qif_unc_conflict{field: "U"}`.
- **AC-6** (error path) Given an extension version `unc-qif-stmt@9`, then `unsupported_extension`.

## Design

- **Registry command(s):** none (library). Algorithm ID `unc.uec.qif_map@1.0.0`.
- **Data structures:** `NativeUncFields { u_std, expanded_u, k, interval, p, unit }` (all optional); `UncQifFragment { native, ext_xml }`; the extension element holds the canonical statement fields with f64 as IEEE-754 hex, so the round trip is exact.
- **Algorithm:** to: build the extension from the canonical statement and fill native fields for UEC-1..3. from: bounded parse of the extension (depth 16, 1 MiB), then rebuild through the validating constructor of `UNC.UEC.STMT` and cross-check native values; with native values only, build the class input with basis `declared` and call `UNC.UEC.CLASS`.
- **Errors:** `qif_unc_conflict{field}`, `unsupported_extension`, `limit_exceeded`, statement constructor errors.
- **Determinism:** D0.
- **Performance budget:** 10 000 statements mapped both ways in under 200 ms [unverified until measured].
- **Crate:** `fid-kunc` (module `uec::qif_map`; no I/O).

## Test fixtures

`fixtures/unc/stmt/valid/` (shared with `UNC.UEC.STMT`) and `fixtures/unc/qif/` hand-written documents (CC-BY-4.0); the pinned QIF 3.0 XSD (artifact `IO.QIF.GEN.PIN`) and the CORE extension schema (artifact `CORE.PRJ.QIF`) for the CI validity check.

## Build prompt

> In crate `fid-kunc`, module `uec::qif_map` (pure, no I/O, no dependency on IO crates), implement `to_qif(&UncertaintyStatement) -> UncQifFragment { native: NativeUncFields, ext_xml }` and `from_qif(&UncQifFragment) -> Result<UncertaintyStatement, QifMapError>`. The extension element `fx:UncertaintyStatement` version `unc-qif-stmt@1` carries every statement field (f64 as IEEE-754 hex) and goes in QIF's generic user-data extension hook. Native fields (standard uncertainty, U, k, interval, p, unit) are filled for UEC-1..3 only; UEC-0 has the marker and reason only. On import the extension is authoritative: native values must match within 1 ulp (else `qif_unc_conflict`); native-only fragments become UEC-1 `DECLARED` with source `qif_import` via `uec::assign`. Bounded parsing (depth 16, 1 MiB); unknown versions give `unsupported_extension`. Publish `qif-unc-map@1` (field, QIF element name, ISO 23952 clause number; no standard text) and the XSD fragment for the extension. Tests AC-1..AC-6. Constraints: CONSTITUTION 5 (sanctioned extensions only), never paste standards text, KNP-1, CONSTITUTION 8.

## Open questions

- **Applied:** the hook is the generic QIF user-data extension (the `UserDataXML` mechanism inside attributes), with native elements filled where they exist. Which native QIF uncertainty elements exist, and their names, must still be confirmed against the pinned XSD when the leaf is built [unverified]; rows without a native element simply carry no native value.
- **Cross-domain (open):** `CORE.PRJ.QIF` owns the `fx` extension schema. It must add `fx:UncertaintyStatement` (`unc-qif-stmt@1`, published here) to its catalogue. `IO.RES.QIFR.ROUT` and `IO.RES.QIFR.RIN` should call this mapping, which is L0 so the dependency is legal, instead of writing only the class.
- **Layering:** `IO.QIF` was removed from `depends_on`. This mapping is L0 and must not depend on L2 IO crates; it reads the XSD and the extension schema as artifacts.
- The import downgrade (REQ-004) may need review by IO owners.

## Children

_Leaf: none._
