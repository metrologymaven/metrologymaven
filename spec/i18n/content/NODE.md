---
id: I18N.CONTENT
title: Localised reports, form variants, docs and tutorials
level: 2
parent: I18N
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-I18N-001, REQ-I18N-004]
covers: []
depends_on: [I18N.CATALOG, I18N.LOCALE, I18N.UNITS, I18N.XLATE, I18N.GLOSS, RPT.MODEL.TMPL, RPT.MODEL.DOC, RPT.FORMS.LAYOUT, OUTR.DOCS.FRAME, OUTR.DOCS.BOOK]
cli: [fid locale set-report-language]
mcp: [locale_set_report_language]
journal: [locale.set_report_language@1]
updated: 2026-11-16
---

# I18N.CONTENT: Localised reports, form variants, docs and tutorials

## Purpose

Applies the catalog, locale and workflow to the product's long-form outputs. A report has its own language, chosen per report and independent of the viewer's UI language, because a German supplier often sends an English report to its customer. This node owns report-language selection, bilingual label variants, localised variants of the fixed industry forms, and the localised editions of the docs book and tutorials with page-level fallback and staleness banners. It exists apart from the catalog because these outputs are documents with their own reproducibility rules.

## Scope

**In:**
- Report language as a property of a report document, set by `locale.set_report_language@1` (W2), recorded with the catalog version and locale-data release used
- Template labels resolved in the report language; optional bilingual labels (report language plus English)
- Localised variants of FAI and PPAP form layouts, within the limits D-015 sets
- Localised docs editions (book, tutorials, migration guides) built per shipped language, with per-page fallback to English and an "outdated translation" banner when the English page changed after translation
- Localised starter report templates

**Out** (and which node owns it instead):
- Catalog format and lookup (`I18N.CATALOG`)
- Number, date and unit rendering (`I18N.LOCALE`, `I18N.UNITS`; applied through `RPT.MODEL.FMT`)
- Translation workflow and thresholds (`I18N.XLATE`)
- Template format and binding (`RPT.MODEL.TMPL`, `RPT.MODEL.BIND`); fixed form layouts themselves (`RPT.FORMS.LAYOUT`)
- Docs portal frame, editions and offline bundles (`OUTR.DOCS.FRAME`); docs text in English (`OUTR.DOCS.BOOK`, `OUTR.CONTENT`)

## Requirements

- **REQ-I18N.CONTENT-001** Each report document shall have a report language, defaulting to the author's UI language, and changing it shall be a journaled W2 command that changes no value of record or decision.
- **REQ-I18N.CONTENT-002** When a report is rendered, every template label shall come from the catalog in the report language, and the rendered report shall record the language, catalog version and locale-data release.
- **REQ-I18N.CONTENT-003** When the same report is rendered twice with the same language, catalog version, locale-data release and renderer version, the outputs shall be byte-identical.
- **REQ-I18N.CONTENT-004** When bilingual labels are enabled, each label shall show the report-language term and the English term, both taken from the catalog.
- **REQ-I18N.CONTENT-005** A localised FAI or PPAP form variant shall carry the same fields as the base form, and if a variant lacks a field present in the base form, then form finalisation shall fail with the field name.
- **REQ-I18N.CONTENT-006** Each shipped language shall have a docs edition built from translated pages, and any page without a current translation shall show the English page with a visible fallback notice.
- **REQ-I18N.CONTENT-007** If an English docs page changes after its translation was reviewed, then the translated page shall show an "outdated translation" banner until it is re-reviewed.
- **REQ-I18N.CONTENT-008** Translated docs pages, template labels and form variants shall pass the `I18N.GLOSS` check before publication.

## Interfaces

- **Offers:** `locale.set_report_language@1` (W2); localised starter templates and form variants; per-language docs sources and the fallback and staleness rules for `OUTR.DOCS.FRAME`.
- **Consumes:** catalog lookup (`I18N.CATALOG`); render transforms (`I18N.LOCALE`); unit defaults (`I18N.UNITS`); workflow and gates (`I18N.XLATE`); glossary check (`I18N.GLOSS`); template format (`RPT.MODEL.TMPL`); report document model (`RPT.MODEL.DOC`); form layouts (`RPT.FORMS.LAYOUT`); docs frame and book (`OUTR.DOCS.FRAME`, `OUTR.DOCS.BOOK`).

## Decomposition hints

RLANG (report language command and recorded versions), LABELS (template label resolution and bilingual mode), FORMS (localised form variants), DOCS (per-language docs editions, fallback, staleness).

## Open questions

- `RPT.FORMS.LAYOUT` is blocked on D-015 (form layout copying). Localised form variants inherit that block; until it resolves, FORMS work here can only cover field labels.
- Whether customers require AS9102 forms in English regardless of supplier language [unverified]; if so, bilingual mode is the only localised variant needed.

## Children

_None yet: status `open`._
