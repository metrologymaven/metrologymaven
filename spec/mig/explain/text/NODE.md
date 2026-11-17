---
id: MIG.EXPLAIN.TEXT
title: Difference explanation templates and view
level: 3
parent: MIG.EXPLAIN
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-MIG.EXPLAIN-006]
covers: []
depends_on: [MIG.EXPLAIN.ATTR, RPT.MODEL.FMT]
side_effect: R0
determinism: D0
cli: [fid correlate show-explanation]
mcp: [correlate_show_explanation]
journal: [correlate.show_explanation@1]
updated: 2026-11-17
---

# MIG.EXPLAIN.TEXT: "Why is it different?" templates and view

## Purpose

Renders an attribution record as short, plain text a quality engineer can read: which factor closed the difference, what our value became under the probed setting, and what the residual was. Text comes only from versioned templates written in our own words, with slots filled from the record's kernel values through the report formatter; this leaf computes no number. Standards are cited by designator and clause only.

## Scope

**In:**
- Template set `mig-why@1`: one template per outcome kind (credited single factor, credited pair, unexplained, agent-assisted banner, adopted banner), per factor
- Slot filling from a `DifferenceAttribution` record with values formatted by `RPT.MODEL.FMT`
- `correlate.show_explanation@1` (R0): returns the record plus rendered text, for one pair or a whole report
- A template lint: no numeric literals in templates, citations only as designator plus clause, slots must exist in the record schema

**Out:**
- Attribution and records (owned by `MIG.EXPLAIN.ATTR`)
- GUI panel layout (owned by UX/VIZ surfaces that call this command)
- Translation (owned by `I18N`, later)

## Requirements

- **REQ-MIG.EXPLAIN.TEXT-001** When `correlate.show_explanation@1` runs for a record, the system shall return the record unchanged and text rendered from the template selected by the record's outcome, factor list and provenance label.
- **REQ-MIG.EXPLAIN.TEXT-002** Every number in the rendered text shall come from a field of the record formatted by the report formatter, and the template lint shall fail any template that contains a numeric literal outside a citation.
- **REQ-MIG.EXPLAIN.TEXT-003** The template lint shall fail any template whose standards citation is not in the form designator, edition and clause, or whose length exceeds 400 characters.
- **REQ-MIG.EXPLAIN.TEXT-004** Rendered text for an `AGENT_ASSISTED` record shall begin with a banner stating that the attribution is agent-assisted and not yet adopted.
- **REQ-MIG.EXPLAIN.TEXT-005** The rendered text shall name the template set version, and the same record and template version shall render byte-identical text.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/mig/explain/text/records.json` (three records from the ATTR fixture), when `fid correlate show-explanation --report <id> --json` runs, then each rendered text equals `expected/<pair>.txt` byte for byte.
- **AC-2** Given an `AGENT_ASSISTED` record, then its text begins with the agent-assisted banner.
- **AC-3** (error path) Given `bad-templates/` (one template containing the literal `0.01`), when the template lint runs, then it fails with `TEMPLATE-NUMBER` naming the template.
- **AC-4** (error path) Given an unknown record ID, then the command fails with `not_found`.

## Design

- **Registry command(s):** `correlate.show_explanation@1`, R0; params `report: ParallelRunReportId`, `pair?: PairId`; result `Vec<{ record: DifferenceAttribution, text: String, template_version }>`.
- **Data structures:** `TemplateSet { version, templates: BTreeMap<TemplateKey, String> }`, `TemplateKey { outcome, factors, provenance }`; slot syntax `{field.path}` resolved against the record schema.
- **Algorithm:** select template by key (fallback order fixed: exact factors → single-factor generic → outcome generic) → fill slots with `RPT.MODEL.FMT` formatted values (unit and decimals as authored) → prepend banner by provenance.
- **Errors:** `not_found`, `TEMPLATE-NUMBER`, `TEMPLATE-CITATION`, `TEMPLATE-SLOT`, `TEMPLATE-LENGTH` (lint, via `cargo xtask mig template-lint`).
- **Determinism:** D0.
- **Performance budget:** render 1,000 records in under 200 ms [unverified until measured].
- **Crate:** `fid-mig` (module `explain::text`).

## Test fixtures

`fixtures/mig/explain/text/`: records copied from the ATTR fixture outputs and expected texts reviewed by hand; templates are original project wording, CC-BY-4.0.

## Build prompt

> In crate `fid-mig`, add module `explain::text` and register `correlate.show_explanation@1` (R0; CLI `fid correlate show-explanation`, MCP `correlate_show_explanation`). Implement spec node MIG.EXPLAIN.TEXT: a versioned template set `mig-why@1` keyed by outcome, factor list and provenance, with `{field.path}` slots filled only from the `DifferenceAttribution` record through the report value formatter (never compute a number); prepend the agent-assisted or adopted banner by provenance; include the template set version. Add `cargo xtask mig template-lint` failing on numeric literals outside citations (`TEMPLATE-NUMBER`), citations not in designator/edition/clause form (`TEMPLATE-CITATION`), unknown slots (`TEMPLATE-SLOT`) and templates over 400 characters (`TEMPLATE-LENGTH`). Write all template wording originally; never paste standards text or incumbent wording. Tests for AC-1..AC-4. No `unwrap`/`expect`/`panic`/`unsafe`.

## Open questions

- None.

## Children

_Leaf: none._
