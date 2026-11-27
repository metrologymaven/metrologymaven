---
id: QUAL.VAP.TPL
title: Template format, URS, risk assessment and PQ guidance templates
level: 3
parent: QUAL.VAP
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-qual, fid-xtask]
built_pr: 654
satisfies: [REQ-QUAL.VAP-001, REQ-QUAL.VAP-008]
covers: []
depends_on: []
uses_artifacts: []
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# QUAL.VAP.TPL: Template format, URS, risk assessment and PQ guidance templates

## Purpose

Owns the format every VAP template is written in and the three templates that frame a customer's validation: the user requirements specification (pre-filled from intended-use selections), the risk assessment (intended use to feature to process risk, choosing scripted or unscripted testing in the style of FDA CSA guidance [unverified guidance details]) and the PQ guidance for the customer's own calibrated-part study. Templates are data (`vap-template/1` JSON) rendered to editable Markdown and ODF Text, so the generator and checks read one source.

## Scope

**In:**
- Template schema `vap-template/1`: document ID, title, version, sections, fields (with type and optional pre-fill rule keyed by intended-use ID), framework references by designator, edition and clause ID only, licence notice
- Intended-use catalogue `vap/intended-uses.toml` (P1a: transcription reports, prismatic GD&T subset, scan inspection)
- Templates `urs`, `risk`, `pq-guidance` and `periodic-review` (the layout used by `QUAL.VAP.REVIEW`)
- Renderer library: template plus selections to Markdown and `.odt`
- Template-set check: required documents present (URS, risk, IQ, OQ, PQ, 17025 checklist, Part 11 and Annex 11 checklist, supplier questionnaire), schema valid, licence notice present

**Out** (and which node owns it instead):
- IQ and OQ protocols (owned by `QUAL.VAP.PROTO`); checklists (owned by `QUAL.VAP.CHECK`); supplier questionnaire (owned by `QUAL.VAP.SUPQ`)
- Writing the pack for a customer (owned by `QUAL.VAP.GEN`)

## Requirements

- **REQ-QUAL.VAP.TPL-001** Every template shall validate against `vap-template/1`, and the check shall fail and name any template that does not.
- **REQ-QUAL.VAP.TPL-002** If the template set lacks any of the eight required documents, then the template-set check shall fail and name it.
- **REQ-QUAL.VAP.TPL-003** The renderer shall produce, from one template and one selection, a Markdown file and an ODF Text file with the same section and field order.
- **REQ-QUAL.VAP.TPL-004** Every template shall carry the licence notice of the documentation licence (CC-BY-4.0 under D-001) and state that customers may modify and reuse it.
- **REQ-QUAL.VAP.TPL-005** When a URS is rendered for a set of intended-use IDs, the renderer shall pre-fill exactly the requirement rows whose pre-fill rule names one of those IDs.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/qual/vap/tpl/set-ok/` (eight templates), when `cargo xtask qual vap check-templates --dir set-ok/` runs, then it exits 0.
- **AC-2** (error path) Given `set-no-supq/`, then it exits 1 naming `supplier-questionnaire`.
- **AC-3** Given template `urs` and selection `[transcription_reports]`, when rendered, then `urs.md` and `urs.odt` equal their golden files and only rows with that rule are pre-filled.
- **AC-4** (error path) Given a template without a licence notice, then the check fails naming it.
- **AC-5** (determinism) Given AC-3 twice, then both outputs are byte-identical (fixed ZIP metadata in `.odt`).

## Design

- **Registry command(s):** none here; library `qual::vap::tpl` (used by `QUAL.VAP.GEN`) and repository check `cargo xtask qual vap check-templates --dir <dir>` (R0). Rendering writes only into a caller-given root (W1 through the generator).
- **Data structures:** `Template { id, title, version, sections: Vec<Section{ id, heading, fields: Vec<Field{ id, kind: Text|Choice|Table|Signature, prefill: Option<Vec<IntendedUseId>> }> }>, refs: Vec<FrameworkRef{ designator, edition, clause }>, licence }`.
- **Algorithm:** schema validation; deterministic Markdown and ODF writers (fixed member order and timestamps).
- **Errors:** `TPL-SCHEMA(template)`, `TPL-MISSING(doc)`, `TPL-LICENCE(template)`.
- **Determinism:** D0.
- **Performance budget:** under 1 s per pack.
- **Crate:** `fid-qual` (module `vap::tpl`); the check is a thin `fid-xtask` wrapper over it.

## Test fixtures

`fixtures/qual/vap/tpl/` with project-written templates; CC-BY-4.0.

## Build prompt

> In crate `fid-qual`, add module `vap::tpl`: the `vap-template/1` types (JSON Schema under `schemas/`), an intended-use catalogue loader, and deterministic renderers to Markdown and ODF Text (fixed ZIP member order and metadata). Add the templates `urs`, `risk` (intended use → feature → process risk → scripted or unscripted test), `pq-guidance` (customer calibrated-part study, never automated) and `periodic-review` as JSON under `vap/templates/`, each with the CC-BY-4.0 licence notice and framework references as designator, edition and clause ID only, no quoted standard text. Add `cargo xtask qual vap check-templates --dir <dir>` checking the schema, the eight required documents and the licence notice. No panics, deny unwrap/expect. Tests: AC-1..AC-5 of spec node QUAL.VAP.TPL.

## Open questions

- Template licence: CC-BY-4.0 follows D-001's documentation licence; CC0 was also proposed and remains a maintainer choice.
- DOCX output may be wanted by customers; ODF is the open format chosen here.
- **Build notes (fid-qual `vap::tpl`, `cargo xtask qual vap check-templates`):** the eight required document IDs are `urs`, `risk`, `iq-protocol`, `oq-protocol`, `pq-guidance` (the PQ document), `checklist-17025`, `checklist-part11-annex11` and `supplier-questionnaire`, taken from PROTO, CHECK and SUPQ. `vap/templates/` holds only this leaf's four templates, so a set check of that folder reports the other four missing until those leaves land. `interpretation` accepts only `provisional` until a template sign-off record exists (D-045), and every render says so. The catalogue uses `[[intended_use]]` tables. Errors beyond the three named here: `TPL-CATALOGUE` (bad catalogue), `TPL-SELECTION` (unknown or repeated selected use) and `TPL-LIMIT` (an `.odt` that would need ZIP64). Table columns are `static`, `prefill` or `customer`; a row's pre-fill cells show only when its rule names a selected use. Golden renders come from the stdlib reference `fixtures/qual/vap/tpl/make_cases.py`.

## Children

_Leaf: none._
