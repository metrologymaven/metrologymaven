---
id: RPT.MODEL.TMPL.FORMAT
title: Template document format, bounded parser and schema migration
level: 4
parent: RPT.MODEL.TMPL
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-report]
built_pr: 268
satisfies: [REQ-RPT.MODEL.TMPL-001, REQ-RPT.MODEL.TMPL-007, REQ-RPT.MODEL.TMPL-008, REQ-RPT.MODEL.TMPL-009]
covers: [GOM.RPT.02, PCD.RPT.01, PW.RPT.02, SA.RPT.01]
depends_on: [RPT.MODEL.DOC, CORE.JRN.CANON]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-19
---

# RPT.MODEL.TMPL.FORMAT: Template document format, bounded parser and schema migration

## Purpose

Defines the exchanged template file: a versioned, canonical-JSON document that is a report document whose blocks may carry slots instead of bound IDs, plus template-level flags (locked, continuation and repetition hints for layout). It owns the bounded, fail-closed parser, the schema check and deterministic migration from older schema versions. Templates are a separate JSON format, not a QIF extension, because QIF's sanctioned extension hooks carry measurement data, not page documents [unverified]; templates reference QIF and project entities only by stable ID.

## Scope

**In:** media type `application/vnd.fid.report-template+json` and schema version 1; document structure (header, pages, blocks with optional `slot`, block flags `repeat_header_rows`, `continuation_block`); template flags `locked`, `clearance`; parser limits; schema validator; migration chain; untrusted typing of every string.

**Out:** slot and rule semantics (`RPT.MODEL.TMPL.SLOT`); versions, storage and commands (`RPT.MODEL.TMPL.VER`); starter content (`RPT.MODEL.TMPL.STARTER`); block payload schemas (block kinds); style sets saved with templates (`RPT.DESIGN.STYLE`).

## Requirements

- **REQ-RPT.MODEL.TMPL.FORMAT-001** A template document shall have the structure of a report document in which any block may carry a `slot` object instead of bound stable IDs, and shall declare `schema_version`, `template_id` and `title`.
- **REQ-RPT.MODEL.TMPL.FORMAT-002** The format shall define the template flag `locked` and the block flags `repeat_header_rows` (0 to 8) and `continuation_block` (a block ID on the same page), and the parser shall reject any other unknown field with error `report.template_invalid`.
- **REQ-RPT.MODEL.TMPL.FORMAT-003** If a template file exceeds 16 MiB, a nesting depth of 32, 10 000 blocks or a string of 64 KiB [unverified limits], or fails schema validation, then loading shall fail with error `report.template_invalid` naming the JSON pointer of the first offending element.
- **REQ-RPT.MODEL.TMPL.FORMAT-004** When a template with an older schema version is loaded, the parser shall migrate it through each intermediate version in order to the current version, and if no migration exists it shall fail with error `report.template_schema_unsupported`.
- **REQ-RPT.MODEL.TMPL.FORMAT-005** Migrating the same input twice shall give byte-identical canonical JSON (RFC 8785).
- **REQ-RPT.MODEL.TMPL.FORMAT-006** Every string in a template shall be parsed into an untrusted-content type and marked untrusted in every CLI and MCP output (CONSTITUTION C6).
- **REQ-RPT.MODEL.TMPL.FORMAT-007** The format specification shall be published as a JSON Schema file with the schema version in its name, so other tools can validate templates without the crate.

## Acceptance criteria

- **AC-1** Given `fixtures/rpt/model/tmpl/valid_v1.json`, when parsed and re-serialised, then the canonical bytes equal the input's canonical form and every string is marked untrusted.
- **AC-2** (error path) Given fixtures `too_large.json` (17 MiB), `deep.json` (depth 33), `unknown_field.json` and `bad_continuation.json` (block on another page), when parsed, then each fails with `report.template_invalid` and the reported JSON pointer.
- **AC-3** Given `fixtures/rpt/model/tmpl/v0_sample.json` (a hand-written pre-release schema 0), when loaded, then it migrates to v1 deterministically (two runs byte-identical); given `schema_version: 99`, it fails with `report.template_schema_unsupported`.
- **AC-4** (fuzz) Given 1 000 000 iterations of a structure-aware fuzzer (seed 3), then the parser never panics and every failure is a typed error.
- **AC-5** Given the published schema file, when `valid_v1.json` and `unknown_field.json` are validated with an off-the-shelf JSON Schema validator, then the first passes and the second fails.

## Design

- **Registry commands:** none; library API `parse_template(bytes) -> Result<Template, TemplateError>`, `serialise(&Template) -> CanonicalBytes`, `migrate(value, from) -> Result<Value, TemplateError>`.
- **Data structures:** `Template { schema_version: u32, template_id: TemplateId, title: Untrusted<Text>, locked: bool, clearance: Option<Clearance>, pages: Vec<TemplatePage> }`; `TemplateBlock { id, kind, kind_version, payload, slot: Option<Slot>, repeat_header_rows: u8, continuation_block: Option<BlockId> }` (`Slot` defined in `RPT.MODEL.TMPL.SLOT`, carried here as validated JSON).
- **Algorithm:** streaming JSON reader with byte, depth, count and string-length budgets; schema check; migration chain `v0 -> v1` (pure functions on canonical JSON).
- **Errors:** `report.template_invalid { pointer }`, `report.template_schema_unsupported`.
- **Determinism:** D0.
- **Performance budget:** parse a 1 MiB template in under 20 ms [unverified until measured].
- **Crate:** `fid-report` (module `tmpl::format`); schema file `schemas/report-template.v1.json`.
- **Pinned at build** (fid-report `tmpl::format`; each rule has a test in `crates/fid-report/tests/tmpl_format.rs`, `tests/tmpl_format_fuzz.rs` or `fixtures/rpt/model/tmpl/check_schema.py`; fixtures and expected bytes from `fixtures/rpt/model/tmpl/make_fixtures.py`):
  - *Schema version 1.* One RFC 8785 document with exactly the keys `clearance`, `locked`, `media_type`, `pages`, `schema_version`, `template_id`, `title`; every key is required (absent values are `null`), so serialising a parsed template gives the canonical form of its input. `media_type` is `application/vnd.fid.report-template+json`; `schema_version` is `"1"`; `template_id`, page and block IDs are canonical lowercase UUIDv7 and all distinct; `title` is `{"text", "untrusted": true}` (`Untrusted<Text>`, 4096 bytes, no NUL). `clearance` is `null`, `{"state": "pending"}` or `{"review_ref": <untrusted text>, "state": "cleared"}`. A page is `{blocks, id}`. A block is `{continuation_block, id, kind, kind_version, payload, repeat_header_rows, slot}`: `kind` matches `^[a-z][a-z0-9_]{0,31}$`; `kind_version` is a decimal string of at least 1; `payload` is `{"untrusted": true, "value": <template json>}`; `repeat_header_rows` is `"0"` to `"8"`; `continuation_block` is `null` or the ID of another block on the same page, and no chain of continuations loops; `slot` is `null` or `{"kind": <slot kind>, "spec": {"untrusted": true, "value": <template json object>}}`. Integers are canonical decimal strings (no leading zero), as in `RPT.MODEL.DOC`, so a v1 template holds no JSON number. Block kinds are not checked against a kind registry here: template exchange must not depend on which kinds a build registers (`RPT.MODEL.TMPL.VER` checks kinds on save and apply).
  - *No values, by construction.* "Template json" (type `TemplateJson`, the only type for payloads and slot specs) holds no JSON number anywhere and no object whose `schema` member is a string starting `value_binding@` (reason `bound_value`) or `value_text@` (reason `value_text`); its constructor is the check, and it keeps the payload bounds of `RPT.MODEL.DOC` (65 536 canonical bytes, reason `value_too_large`). No template type has a field for a record, a digest or a value. Templates name QIF and project entities only by stable ID inside slot specs.
  - *Slot kinds.* The closed list of REQ-RPT.MODEL.TMPL.SLOT-001: `characteristics`, `elements`, `tags`, `stages`, `runs`, `saved_query`, `field_table`; any other is `unknown_slot_kind`. The spec's inner shape (rule, sort, repetition) belongs to `RPT.MODEL.TMPL.SLOT`.
  - *Budgets.* File 16 MiB (reason `too_large`, pointer `""`); depth 32, counting the root container as 1 and scalars as nothing (`too_deep`, pointer of the first container past the budget); strings, keys included, 65 536 bytes of decoded UTF-8 (`string_too_long`); 10 000 blocks over all pages (`too_many_blocks`, pointer of the 10 001st block) and 10 000 pages (`too_many_pages`).
  - *Order of checks.* Byte budget; then one pre-scan of the raw bytes in input order for depth, string length and duplicate keys (`duplicate_key`); then `fid-canon`'s strict parse (`syntax`, `invalid_utf8`, `non_finite_number`, each naming the element that holds the failing byte); then the schema version; then migration; for a migrated value, the pre-scan again over its canonical bytes; then the v1 check. The v1 check visits each object's key set first (first unknown key in code-point order is `unknown_field`, then first missing key `missing_field`), then members in code-point key order, depth first, pages and blocks in index order; a page's continuation flags are checked once all its blocks are read (`continuation_self`, `continuation_not_on_page`, then `continuation_cycle` at the first block in page order that lies on a loop). Other reasons: `wrong_type`, `bad_id`, `repeated_id` (at the later occurrence in that order), `bad_kind`, `bad_integer`, `number`, `out_of_range`, `bad_version`, `bad_media_type`, `bad_marker`, `bad_clearance`, `text_too_long`, `text_nul`, `bad_format`.
  - *Errors.* `report.template_invalid {pointer, reason}`: the pointer is RFC 6901 (`~0`, `~1`), built from input keys and therefore untrusted: its machine form is `{"text", "untrusted": true}`, its human form escapes what `Untrusted::escaped` escapes, and it is cut at 1024 bytes. `report.template_schema_unsupported {schema_version}` for a version above the current one.
  - *Schema version detection.* `schema_version` of the root object, read before anything else, as a canonical decimal string or a JSON integer (0 to 2^32 - 1); anything else is `bad_version`; a missing one `missing_field`; a non-object root `wrong_type`. A v1 document must then spell it `"1"`.
  - *Migration.* `migrate(value, from)`: identity for the current version, `report.template_schema_unsupported` above it, otherwise each step in order. Steps are pure functions of the value (no clock, randomness or ID drawing). Errors found during a step name the pointer in that step's input; errors found by the v1 check after migration name the pointer in the migrated document.
  - *Schema version 0* (hand-written pre-release form): `{"format": "fid.report-template", "schema_version": 0, "id", "name": <string>, "locked"?: <bool>, "pages": [{"id", "blocks": [{"id", "kind", "version": <JSON integer>, "data": <json>, "slot"?: null | {"kind", "rule": <object>}, "repeat_header"?: <JSON integer>}]}]}`; unknown keys are `unknown_field`, another `format` is `bad_format`, a non-integer `version` or `repeat_header` is `bad_integer`. Step 0 to 1: drop `format`; `id` to `template_id`; `name` to `title` (wrapped untrusted); `locked` defaults to `false`; add `clearance: null` and `media_type`; per block `version` to the decimal string `kind_version`, `data` to `payload` (wrapped), a slot's `rule` to `spec` (wrapped), `repeat_header` (default 0) to the decimal string `repeat_header_rows`, add `continuation_block: null`; `schema_version` to `"1"`. Values are moved, never reinterpreted, so a v0 payload holding a number fails the v1 check.
  - *API.* `parse_template(&[u8]) -> Result<Template, TemplateError>`; `serialise(&Template) -> Result<CanonicalBytes, TemplateError>` (a `Result` because the canonical writer is fallible; it cannot fail for a parsed template, which holds no number); `migrate(&CValue, u32) -> Result<CValue, TemplateError>`; `TemplateJson::new(&CValue, pointer)`.
  - *JSON Schema.* `schemas/report-template.v1.json` (draft 2020-12) states the v1 structure, the no-number and no-value-tag rule for template json, and `x-untrusted: true` on every untrusted wrapper; its description lists the rules JSON Schema cannot express (budgets, distinct IDs, u32 range, continuation targets, duplicate keys).
  - *Fixtures.* `fixtures/rpt/model/tmpl/`: `valid_v1.json` (non-canonical spelling, hostile strings) and its canonical bytes, `v0_sample.json` and its migrated bytes, `deep.json`, `unknown_field.json`, `bad_continuation.json`, and `invalid_cases.json` (hand-labelled code, pointer, reason and schema verdict). The 17 MiB file of AC-2 is built in the test (trailing whitespace after `valid_v1.json`) rather than committed.
  - *Fuzz.* AC-4 runs as an ordinary test: SplitMix64 seed 3, 1 000 000 inputs, each a fixture with one to three tree mutations and, one time in three, a byte mutation; every success must round-trip byte-identically.

## Test fixtures

`fixtures/rpt/model/tmpl/` (valid, invalid and migration cases, hand-written, CC-BY-4.0); fuzz corpus seeded from the valid fixtures.

## Build prompt

> In crate `fid-report`, add module `tmpl::format`: the report-template file format v1 (`application/vnd.fid.report-template+json`), a separate canonical JSON format, not a QIF extension. Structure: report document plus optional per-block `slot`, template flags `locked` and `clearance`, block flags `repeat_header_rows` (0-8) and `continuation_block` (same-page block ID). Bounded fail-closed parser (16 MiB, depth 32, 10 000 blocks, 64 KiB strings; unknown fields rejected) returning `report.template_invalid` with a JSON pointer; deterministic migration chain (v0 -> v1) or `report.template_schema_unsupported`; all strings `Untrusted`; RFC 8785 serialisation; publish `schemas/report-template.v1.json`. Acceptance: AC-1..AC-5 in `spec/rpt/model/tmpl/format/NODE.md`, including a 1M-iteration fuzz run. Constraints: CONSTITUTION C6, §8 (all input hostile, no panic, no `unsafe`, deny `unwrap`/`expect`).

## Open questions

- **Decided here (technical, from the parent's open question):** templates use a separate JSON format; nothing in the QIF exchange contract changes, so no decision record is needed. If maintainers later want templates inside QIF files, that would change the exchange contract and needs a decision.
- Parser limits are placeholders [unverified].

## Children

_Leaf: none._
