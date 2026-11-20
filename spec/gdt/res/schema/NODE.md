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
built_in: [fid-gps]
built_pr: 226
satisfies: [REQ-GDT.RES-002, REQ-GDT.RES-005]
covers: []
depends_on: [KRN.NUM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-19
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
- **Algorithm:** `fid-canon`'s strict bounded JSON parser with an explicit closed field set on every object (no serde); canonicalise with `fid-canon`'s RFC 8785 writer; f64 as hex strings; SHA-256.
- **Errors:** `bad_dmt_key`, `duplicate_dmt_key`, `unknown_operator_kind`, `bad_algorithm_id`, `field_too_long`, `malformed_table`.
- **Determinism:** D0; parsing and hashing involve no floating-point arithmetic.
- **Performance budget:** load and digest a 5 000-row table in < 200 ms [unverified].
- **Crate:** `fid-gps` (GD&T library, CONSTITUTION §2.1 L0; layer `model` in `layers.toml`; module `dmt`; pure, no I/O: callers pass bytes).
- **Pinned choices (as built in `fid-gps`, module `dmt`):**
  - *Layer:* `fid-gps` is in layer `model` of `layers.toml` (CONSTITUTION §2.1 L0, beside canonical JSON), not `kernel`: RFC 8785 and SHA-256 come from `fid-canon` (model) and `sha2`, which the zero-dependency kernel graph cannot reach. KNP-1 still holds in substance: no floating-point arithmetic, no `HashMap`/`HashSet`, no panics. Parsing uses `fid-canon`'s strict bounded parser, not serde; `deny_unknown_fields` is an explicit field-set check on every object.
  - *Table document:* `{schema: "fid.dmt-table/1", version: "x.y.z", rows: [...]}`, every field required and no other; at most 32 MiB, nesting depth 8, 10 000 rows; no duplicate member name in any object (`malformed_table{duplicate_key}`). Rows may come in any order; everything else is fixed. The `schema` string is the schema version and changes on any change to the row format.
  - *Row:* the 16 fields `key, dialect, editions, characteristic, modifiers, clauses, variants, default_variant, actual_value, decision_rule_hint, ors_cases, status, limitations, review, notes, synthetic`, all required, no others. `dialect`, `characteristic` and `modifiers` must agree with the key (`malformed_table{key_mismatch}`); `default_variant` must name a variant.
  - *No JSON numbers anywhere.* Operator parameters (at most 32 per operator, names `[a-z][a-z0-9_]{0,31}`) are a boolean or a string: `0x` + 16 lower-case hex digits is an IEEE-754 binary64 bit pattern (NaN and infinities refused; -0 kept distinct), `0` or `-?[1-9][0-9]*` is an i64, `[a-z][a-z0-9_.-]{0,63}` is a token. The canonical form writes them back unchanged.
  - *Sets:* editions (1 to 8), modifiers (0 to 8), clauses (1 to 16, ordered by designator, edition, clause), variants (1 to 16, by name), ORS cases (0 to 64), limitations (0 to 32) and reviewers (1 to 16) must be strictly ascending by bytes in the file (`malformed_table{not_sorted}`), so the canonical form never reorders an author's list. Chains may be empty (0 to 64 operators), as `GDT.RES.ROWISO.STUBS` placeholders need.
  - *Grammars (all lower-case ASCII, so case-only duplicates cannot both be valid):* key `dmt:<asme|iso>/<characteristic>/<modifiers|none>@<v>` with characteristic `[a-z][a-z0-9_]{0,31}`, modifiers `[a-z0-9][a-z0-9_]{0,15}` (not `none`) strictly ascending and `+`-joined, `v` a u32 of at least 1 without leading zero, at most 128 bytes (so every key is also a defaults-profile key); editions, designators and clause editions 1 to 32 bytes of `[a-z0-9.-]` with alphanumerics at both ends and no two separators in a row; clause numbers `[0-9a-z]+(.[0-9a-z]+)*` up to 24 bytes (annex letters lower case); variant names `[a-z][a-z0-9_]{0,31}`; algorithm IDs three such names and a semver after `@`, up to 128 bytes; actual-value and decision-rule hint IDs dotted names plus `@<u32>`, up to 96 bytes; ORS case IDs `[a-z0-9][a-z0-9._:/-]{0,95}`; limitation codes `[a-z][a-z0-9_]{0,47}` with an optional `:[a-z0-9_]{1,31}` argument; reviewer IDs the roster grammar `[a-z0-9][a-z0-9._-]{0,63}`.
  - *Free text:* `notes` is the only free-text field: at most 280 Unicode scalar values and no control characters (may be empty). Every string is length-checked before its grammar, so an over-long value is `field_too_long{field}` rather than a grammar error.
  - *Encapsulation:* no public type can hold a value the loader would reject. `DmtRow`, `OperatorChain`, `Operator`, `DmtTable`, `ClauseRef`, `Reviewers` (the `signed_off` list) and `SemVer` have private fields and read-only getters, so cached digests cannot go stale. `ClauseRef::new`, `Reviewers::new` and `SemVer::new` (each part at most 999 999 999, the parser's nine digits) apply the loader's rules. A row is built only by `row_from_value`, an operator only by `Operator::new` (same parameter rules as the loader), a chain only by `OperatorChain::new`; `DmtTable::new` re-validates every row by re-parsing its JSON form (`malformed_table{row_invalid}`) and checks it sits under its own key.
  - *Review:* `{state: draft}`, `{state: under_review}` or `{state: signed_off, reviewers: [...]}`; `reviewers` is refused on the first two.
  - *Error order:* table fields, then rows in file order, each row fully checked (in the field order of `row_from_value`) before its key is compared with earlier rows, so the first failure in file order wins. Grammar failures of the key, operator kind and algorithm ID use their own codes; every other failure is `malformed_table{reason}`.
  - *Digests:* chain = SHA-256 of the RFC 8785 form of `{steps: [{alg, kind, params}], variant}`; row = of the row object with all 16 fields; table = of `{rows: [rows by key, byte order], schema, version}`. Written `sha256:<hex>` (`fid-types`).
  - *JSON Schema:* `crates/fid-gps/schema/dmt-table-v1.schema.json` (2020-12), exported as `DMT_TABLE_JSON_SCHEMA`. It checks field sets and grammars; the loader is stricter (sorting, key agreement, unique keys). CI validates both fixtures with Python `jsonschema` and checks hand-labelled mutations fail (`fixtures/gdt/dmt/check_schema.py`).
  - *Defaults-profile key vocabulary (`fid.dmt-profile-keys/1`):* for a table and a dialect, the key of every row of that dialect with two or more variants (value: text naming one of that row's variants) plus the closed named keys `gdt.datum.label_pattern` (GDT.DRF.DATUM), `scan.edge.extraction_method` (SCAN.EDGE.EST.DEFS) and `unc.decision_rule_hint` (GDT.RES.ROWASME.DPROF), each a text value. Adding a named key bumps the vocabulary version. `GDT.CHAR.DPROF` builds its `KeyVocabulary` from this (`KeyVocabulary::from_dmt_table`).

## Test fixtures

`synthetic_table_v1.json`: ~20 invented rows (characteristic names real, clause numbers and choices fictitious, marked `synthetic: true`), CC0, in-repo. No standards text and no expert content.

## Build prompt

> In crate `fid-gps`, module `dmt` (layer `model`: pure, no I/O; no floating-point arithmetic, no std `HashMap`/`HashSet`), define the Dialect Mapping Table schema: `DmtKey` (grammar `dmt:<asme|iso>/<characteristic>/<modifiers|none>@<u32>`, modifiers sorted and `+`-joined), `ClauseRef { designator, edition, clause }`, `Operator { kind: Partition|Extraction|Filtration|Reconstruction|Association|Collection|Construction|Reduction|Evaluation, alg: AlgId, params }`, `OperatorChain { variant, steps }`, `DmtRow` with the fields in REQ-001, `DmtTable { version, rows: BTreeMap }`. Parse with `fid-canon`'s strict bounded parser and reject unknown fields on every object (no serde); free-text fields capped at 280 chars (`field_too_long`). Digests: SHA-256 of RFC 8785 canonical JSON, f64 as IEEE-754 hex strings. Export a JSON Schema 2020-12. Errors: `bad_dmt_key`, `duplicate_dmt_key`, `unknown_operator_kind`, `bad_algorithm_id`, `field_too_long`, `malformed_table`. Tests: synthetic table loads with a fixed digest; order-independence; each error; schema validates the fixture; digest identical across CI platforms. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), no std `HashMap`/`HashSet`, no float arithmetic, no standards text in code, comments or fixtures.

## Open questions

- The 280-character free-text cap is a guess; reviewers (D-009) should confirm it with `GDT.RES.LINT`.
- `fid-gps` sits in layer `model`, not `kernel`, because hashing needs `fid-canon` and `sha2`. If the GD&T evaluators that later join `fid-gps` must be in the KNP-1 kernel graph, the DMT schema should move to its own model-layer crate, or `sha2` be allowlisted (ECO.REPO.ALLOW).

## Children

_Leaf: none._
