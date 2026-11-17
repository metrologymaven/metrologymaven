---
id: IO.RES.TRANS.VIEW
title: Transcribed-status marking for views and exports
level: 4
parent: IO.RES.TRANS
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-IO.RES.TRANS-005]
covers: []
depends_on: [IO.RES.TRANS.MODEL]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.RES.TRANS.VIEW: Transcribed-status marking for views and exports

## Purpose

Makes "never presented as re-evaluated" checkable. It offers the one canonical marking of a transcribed result (`record_status: transcribed` plus source software name and version) in each shape a consumer needs (JSON fields, table columns, a short text label), and a conformance kit that every exporter and every CLI or MCP view of results runs against its own output.

## Scope

**In:**
- `TranscriptionMark { record_status, source_software, source_version }` derived from a result and its record
- Renderers: JSON fragment for CLI and MCP, column values for tables, a one-line label for free-text fields
- Conformance kit `assert_marked(output, results)` with per-format extractors registered by each exporter

**Out:**
- The exporters and views themselves (`IO.RES.CSV.OUT`, `IO.RES.QIFR.ROUT`, `IO.RES.AQDEF.OUT`, `IO.RES.PIWEB`, RPT and GDT views)

## Requirements

- **REQ-IO.RES.TRANS.VIEW-001** For every transcribed result, the JSON fragment shall contain `record_status: "transcribed"`, `source_software` and `source_version`.
- **REQ-IO.RES.TRANS.VIEW-002** The one-line label shall read `transcribed from <software> <version>` with the software and version escaped as untrusted text and truncated to 64 characters each.
- **REQ-IO.RES.TRANS.VIEW-003** When an exporter runs the conformance kit, the kit shall fail with `transcription_unmarked` naming the result ID for any transcribed result whose output lacks the marking.
- **REQ-IO.RES.TRANS.VIEW-004** For a result that is not transcribed, the renderers shall emit `record_status` from the result unchanged and never the word `transcribed`.

## Acceptance criteria

- **AC-1** Given a transcribed result from VendorX 4.2, then the JSON fragment equals `fixtures/io/res/trans/view/mark.json` and the label is `transcribed from VendorX 4.2`.
- **AC-2** Given a software name of 300 characters containing a newline, then the label holds 64 escaped characters and no newline.
- **AC-3** (error path) Given a test exporter output where one of three transcribed results lacks the column, then the kit fails `transcription_unmarked` with that result's ID.
- **AC-4** Given an evaluated (not transcribed) result, then no renderer output contains `transcribed`.

## Design

- **Registry commands:** none (library and test kit).
- **Data structures:** `TranscriptionMark`; `trait MarkExtractor { fn marks(&self, output: &[u8]) -> Result<Vec<(ResultId, Mark)>, IoError> }`.
- **Algorithm:** derive marks; extractors parse each exporter's output; compare sets.
- **Errors:** `transcription_unmarked{result}`.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-io-res` (module `trans::view`).

## Test fixtures

`fixtures/io/res/trans/view/` (CC-BY-4.0).

## Build prompt

> In crate `fid-io-res`, add module `trans::view`: `TranscriptionMark` (record status, source software, source version) with a JSON fragment renderer, table-column values and a one-line label `transcribed from <software> <version>` (untrusted text escaped, each part ≤ 64 characters); a conformance kit `assert_marked` with a `MarkExtractor` trait each exporter implements, failing `transcription_unmarked{result}` when a transcribed result is unmarked. Non-transcribed results pass their status through and never say `transcribed`. CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-4.

## Open questions

- CLI and MCP views of results live in GDT and RPT (later layers); they should call this kit in their own tests. Raised for `GDT.CHAR.RECORD` (`gdt.get_result@1`) and RPT's results catalogue.

## Children

_Leaf: none._
