---
id: RPT.MODEL.TMPL
title: Template format, slots, repetition and starter templates
level: 3
parent: RPT.MODEL
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.MODEL-011, REQ-RPT.MODEL-013, REQ-RPT.MODEL-014, REQ-RPT.MODEL-015]
covers: [GOM.RPT.02, PCD.RPT.01, PW.RPT.02, SA.RPT.01]
depends_on: [RPT.MODEL.DOC, CORE.STO]
cli: [fid report save-template, fid report apply-template, fid report show-template, fid report list-template]
mcp: [report_save_template, report_apply_template, report_show_template, report_list_template]
journal: [report.save_template@1, report.apply_template@1]
updated: 2026-11-16
---

# RPT.MODEL.TMPL: Template format, slots, repetition and starter templates

## Purpose

A template is a report document whose blocks hold **slots** (binding rules) instead of bound IDs. This module owns the template file format, the slot and repetition-rule language, template versioning and immutability, and the built-in starter templates. It is separate from binding (`RPT.MODEL.BIND`) because the format is a published, versioned contract that users exchange, while binding is the engine that reads it.

## Scope

**In:**
- Template document format with a schema version, stored as immutable content addressed by digest
- Slot model: binding rule (by characteristic ID, element ID, tag, stage, run, or catalog query reference), sort key, repetition (one row per item, or one page group per item)
- Template versioning: per-template monotonically increasing version
- Save-as-template from a report and apply-template to create a report
- Bounded, fail-closed template parser and schema migration
- Built-in starter templates: characteristic table, deviation summary, multi-stage comparison table

**Out** (and which node owns it instead):
- Resolving slots against the journal (`RPT.MODEL.BIND`)
- Block payload schemas (`RPT.MODEL.BLOCKS`)
- Page layout and style sets saved with templates (`RPT.DESIGN`)
- Fixed AS9102 and PPAP templates (`RPT.FORMS`)

## Requirements

- **REQ-RPT.MODEL.TMPL-001** A template shall be a report document in which any block may carry a slot holding a binding rule instead of bound stable IDs.
- **REQ-RPT.MODEL.TMPL-002** A binding rule shall select items by characteristic ID, element ID, tag, stage, run or a saved catalog query reference, and shall name a sort key.
- **REQ-RPT.MODEL.TMPL-003** A slot with a repetition rule shall declare whether it emits one table row or one page group per selected item, in the rule's sort order, defaulting to ascending characteristic ID.
- **REQ-RPT.MODEL.TMPL-004** If two selected items have equal sort keys, then the ordering shall fall back to ascending stable record ID so that the order is total.
- **REQ-RPT.MODEL.TMPL-005** When the user saves a template, the system shall assign the next version number for that template ID and store the version as immutable content addressed by its SHA-256 digest.
- **REQ-RPT.MODEL.TMPL-006** When a report is created from a template, the report shall record the template ID, version and digest.
- **REQ-RPT.MODEL.TMPL-007** If a template file exceeds 16 MiB or a nesting depth of 32 [unverified limits], or fails schema validation, then loading shall fail with error `report.template_invalid` naming the first offending element, and journal nothing.
- **REQ-RPT.MODEL.TMPL-008** When a template with an older schema version is loaded, the system shall migrate it deterministically to the current schema or fail with error `report.template_schema_unsupported`.
- **REQ-RPT.MODEL.TMPL-009** All template strings shall be typed as untrusted content (CONSTITUTION C6).
- **REQ-RPT.MODEL.TMPL-010** The system shall ship at least three starter templates (characteristic table, deviation summary, multi-stage comparison table), and each shall generate without warnings against the reference fixture project.

## Interfaces

- **Offers:** `report.save_template@1` (W2), `report.apply_template@1` (W2), `report.show_template@1` (R0), `report.list_template@1` (R0); types `Template`, `Slot`, `BindingRule`, `RepetitionRule`, `TemplateRef`; the template format specification (versioned).
- **Consumes:** report document types (`RPT.MODEL.DOC`), content-addressed storage (`CORE.STO`).

## Decomposition hints

FORMAT (schema, parser, migration), SLOT (binding and repetition rule language), VER (versioning and immutability, save and apply commands), STARTER (built-in templates).

## Open questions

- QIF extension or separate JSON format for templates? QIF's sanctioned extension hooks (CONSTITUTION §5) may not fit page layout. Decide at the FORMAT leaf; open a decision if it changes the exchange contract.
- Should templates be shareable across projects (a user template library in settings)? Assumed yes, stored by digest.

## Children

_None yet: status `open`._
