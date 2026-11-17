---
id: GDT.RES.SCHEMA
title: DMT row and operator-chain schema
level: 3
parent: GDT.RES
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.RES-002, REQ-GDT.RES-005]
covers: []
depends_on: [KRN.NUM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GDT.RES.SCHEMA: DMT row and operator-chain schema

## Purpose

Defines the **structure** of the Dialect Mapping Table (DMT, D-009) and of the operator chains it yields: the row key grammar, the fields every row has, the ISO 17450 operator vocabulary as a closed enum, the table container with version and digest, and the canonical-JSON digest rules. It deliberately contains no interpretation: rows are authored by experts (`GDT.RES.ROWASME`, `GDT.RES.ROWISO`) against this schema. Splitting structure from content lets agents build and test everything around the table before a single expert-authored row exists.

## Scope

**In:** `DmtRow`, `DmtKey`, `ClauseRef`, `OperatorChain`, `Operator`, `RowStatus`, `Variant`, `ReviewState` types; JSON Schema 2020-12 export; key grammar `dmt:<dialect>/<characteristic>/<modifiers>@<v>` with modifiers sorted and `+`-joined (or `none`); `DmtTable` container (version, rows sorted by key); digests (row, chain, table) as SHA-256 over RFC 8785 canonical JSON; the allowed defaults-profile key vocabulary.
**Out:** row content (owned by `GDT.RES.ROWASME`, `GDT.RES.ROWISO`); lookup and variant selection (owned by `GDT.RES.RESOLVE`); lint rules (owned by `GDT.RES.LINT`); proposal and review workflow (owned by `GDT.RES.ROWFLOW`).

## Requirements

- **REQ-GDT.RES.SCHEMA-001** A `DmtRow` shall contain: key, dialect, edition set, characteristic type, modifier set, one or more clause references (designator, edition, clause number), at least one named variant each holding an operator chain, a default variant name, an actual-value definition ID, a decision-rule hint ID, ORS case IDs, status (`supported`, `partial` or `unsupported`), limitations (list of codes) and review state.
- **REQ-GDT.RES.SCHEMA-002** `ClauseRef` shall hold only designator, edition and clause number, and the schema shall offer no field that can hold more than 280 characters of free text [unverified: limit to be confirmed by reviewers].
- **REQ-GDT.RES.SCHEMA-003** `Operator.kind` shall be a closed enum of the ISO 17450 operator vocabulary (partition, extraction, filtration, reconstruction, association, collection, construction, reduction, evaluation), and each operator shall carry an algorithm ID of the form `<module>.<feature>.<method>@<semver>` and canonical parameters.
- **REQ-GDT.RES.SCHEMA-004** The chain digest, row digest and table digest shall each be the SHA-256 of the RFC 8785 canonical JSON of the object, with f64 parameters serialised as IEEE-754 hex strings.
- **REQ-GDT.RES.SCHEMA-005** If a key string does not match the key grammar, or its modifiers are not in sorted canonical order, then parsing shall fail with `bad_dmt_key`.
- **REQ-GDT.RES.SCHEMA-006** If a table contains two rows with the same key, then loading shall fail with `duplicate_dmt_key`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/dmt/synthetic_table_v1.json` (placeholder rows with invented clause numbers, no standards text), when loaded, then all rows parse and the table digest equals the value recorded in `synthetic_table_v1.digest`.
- **AC-2** Given the same table with keys in shuffled file order, when loaded, then the table digest is unchanged.
- **AC-3** (error path) Given key `dmt:asme/position/mmc+cz@1` with modifiers out of order relative to the canonical sort, then parsing fails with `bad_dmt_key`.
- **AC-4** (error path) Given two rows with the same key, then loading fails with `duplicate_dmt_key`.
- **AC-5** (error path) Given an operator with `kind: "smoothing"`, then loading fails with `unknown_operator_kind`.
- **AC-6** Given the exported JSON Schema, when validated with a JSON Schema 2020-12 validator against the fixture, then validation passes; the same digest is produced on Windows, Linux and macOS x86-64 and aarch64 (D0).

## Design

- **Registry command(s):** none; types used by `gdt.resolve_chain@1`, `gdt.get_dmt_row@1` and the matrix generator.
- **Data structures:** as in REQ-001..003. `DmtTable { version: SemVer, rows: BTreeMap<DmtKey, DmtRow> }`. `ReviewState { draft, under_review, signed_off{reviewers: [ReviewerId]} }` (sign-off rules per D-009 are enforced by `GDT.RES.ROWFLOW`, not here).
- **Algorithm:** serde with `deny_unknown_fields`; canonicalise with an RFC 8785 serializer; f64 as hex strings; SHA-256.
- **Errors:** `bad_dmt_key`, `duplicate_dmt_key`, `unknown_operator_kind`, `bad_algorithm_id`, `field_too_long`, `malformed_table`.
- **Determinism:** D0; parsing and hashing involve no floating-point arithmetic.
- **Performance budget:** load and digest a 5 000-row table in < 200 ms [unverified].
- **Crate:** `fid-gps` (kernel GD&T crate, module `dmt`; pure, no I/O: callers pass bytes).

## Test fixtures

`synthetic_table_v1.json`: ~20 invented rows (characteristic names real, clause numbers and choices fictitious, marked `synthetic: true`), CC0, in-repo. No standards text and no expert content.

## Build prompt

> In crate `fid-gps`, module `dmt` (kernel: pure, no I/O, KNP-1), define the Dialect Mapping Table schema: `DmtKey` (grammar `dmt:<asme|iso>/<characteristic>/<modifiers|none>@<u32>`, modifiers sorted and `+`-joined), `ClauseRef { designator, edition, clause }`, `Operator { kind: Partition|Extraction|Filtration|Reconstruction|Association|Collection|Construction|Reduction|Evaluation, alg: AlgId, params }`, `OperatorChain { variant, steps }`, `DmtRow` with the fields in REQ-001, `DmtTable { version, rows: BTreeMap }`. Use serde with `deny_unknown_fields`; free-text fields capped at 280 chars (`field_too_long`). Digests: SHA-256 of RFC 8785 canonical JSON, f64 as IEEE-754 hex strings. Export a JSON Schema 2020-12. Errors: `bad_dmt_key`, `duplicate_dmt_key`, `unknown_operator_kind`, `bad_algorithm_id`, `field_too_long`, `malformed_table`. Tests: synthetic table loads with a fixed digest; order-independence; each error; schema validates the fixture; digest identical across CI platforms. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no std `HashMap`/`HashSet`, no FMA), no standards text in code, comments or fixtures.

## Open questions

- The 280-character free-text cap is a guess; reviewers (D-009) should confirm it with `GDT.RES.LINT`.

## Children

_Leaf: none._
