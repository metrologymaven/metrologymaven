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
satisfies: [REQ-RPT.MODEL.TMPL-001, REQ-RPT.MODEL.TMPL-007, REQ-RPT.MODEL.TMPL-008, REQ-RPT.MODEL.TMPL-009]
covers: [GOM.RPT.02, PCD.RPT.01, PW.RPT.02, SA.RPT.01]
depends_on: [RPT.MODEL.DOC, CORE.JRN.CANON]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
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

## Test fixtures

`fixtures/rpt/model/tmpl/` (valid, invalid and migration cases, hand-written, CC-BY-4.0); fuzz corpus seeded from the valid fixtures.

## Build prompt

> In crate `fid-report`, add module `tmpl::format`: the report-template file format v1 (`application/vnd.fid.report-template+json`), a separate canonical JSON format, not a QIF extension. Structure: report document plus optional per-block `slot`, template flags `locked` and `clearance`, block flags `repeat_header_rows` (0-8) and `continuation_block` (same-page block ID). Bounded fail-closed parser (16 MiB, depth 32, 10 000 blocks, 64 KiB strings; unknown fields rejected) returning `report.template_invalid` with a JSON pointer; deterministic migration chain (v0 -> v1) or `report.template_schema_unsupported`; all strings `Untrusted`; RFC 8785 serialisation; publish `schemas/report-template.v1.json`. Acceptance: AC-1..AC-5 in `spec/rpt/model/tmpl/format/NODE.md`, including a 1M-iteration fuzz run. Constraints: CONSTITUTION C6, §8 (all input hostile, no panic, no `unsafe`, deny `unwrap`/`expect`).

## Open questions

- **Decided here (technical, from the parent's open question):** templates use a separate JSON format; nothing in the QIF exchange contract changes, so no decision record is needed. If maintainers later want templates inside QIF files, that would change the exchange contract and needs a decision.
- Parser limits are placeholders [unverified].

## Children

_Leaf: none._
