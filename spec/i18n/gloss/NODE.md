---
id: I18N.GLOSS
title: Metrology terminology glossaries and the glossary check
level: 2
parent: I18N
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-I18N-005]
covers: []
depends_on: [I18N.CATALOG]
cli: [fid locale show-term, fid locale check-glossary]
mcp: [locale_show_term, locale_check_glossary]
journal: []
updated: 2026-11-16
---

# I18N.GLOSS: Metrology terminology glossaries and the glossary check

## Purpose

Metrology translations fail on terms, not grammar: "flatness", "datum reference frame", "maximum material requirement" and the GD&T symbol names each have one accepted rendering per language, and an inconsistent one makes a report unusable to an auditor. This node owns the per-language reviewed glossary and the check that every translation uses the glossary's term. It is separate from the workflow because the glossary is reference data with its own reviewers (domain experts), and other nodes (docs, reports) check against it too.

## Scope

**In:**
- Glossary data per language: term ID, English term, target term, part of speech, forbidden variants, context note, and the standard designator and clause that defines the concept (designator and clause only, CONSTITUTION §5)
- Coverage: GD&T characteristic and modifier names (ASME Y14.5 and ISO 1101 vocabularies), datum terms, uncertainty and decision-rule terms (ISO 14253-1, JCGM 100), measurement-equipment terms, and project terms such as "Dialect Mapping Table" (D-009)
- Review by at least one domain expert per language, with a review date per entry
- The glossary check over catalog translations, docs and report-template labels
- R0 commands to look up a term and to run the check

**Out** (and which node owns it instead):
- Translation workflow and release gates (`I18N.XLATE`)
- English vocabulary and claim-word lint (`ECO.RULES.VOCAB`)
- GD&T symbol glyph rendering (`VIZ.OVERLAY.GDTGLYPH`)
- Standards text of any kind (never stored)

## Requirements

- **REQ-I18N.GLOSS-001** Each shipped language shall have a glossary covering every term ID in the English base glossary, and each entry shall record a reviewer and review date.
- **REQ-I18N.GLOSS-002** A glossary entry shall cite its defining standard by designator, edition and clause only, and the glossary check shall fail on any entry field longer than 200 characters to keep definitions out [unverified bound].
- **REQ-I18N.GLOSS-003** If a translation of a message that contains an English glossary term does not contain the language's glossary term, then the glossary check shall report an error with the language, message ID and term ID.
- **REQ-I18N.GLOSS-004** If a translation contains a forbidden variant of a glossary term, then the glossary check shall report an error naming the preferred term.
- **REQ-I18N.GLOSS-005** When a glossary entry changes, the check shall list every message, docs page and template label in that language that uses the entry, so they can be re-reviewed.
- **REQ-I18N.GLOSS-006** Where a GD&T term has different names under ASME and ISO dialects in a language, the glossary shall hold one entry per dialect, and the check shall use the dialect recorded on the message or template.

## Interfaces

- **Offers:** glossary files and schema, the glossary check (repository tool and R0 `locale.check_glossary@1`), R0 `locale.show_term@1`.
- **Consumes:** catalog files (`I18N.CATALOG`).
- **Consumed by:** `I18N.XLATE` (merge gate), `I18N.CONTENT` (docs and templates), translation platform exports.

## Decomposition hints

BASE (English base glossary and term IDs), DATA (per-language entries and review), CHECK (matcher with inflection handling per language), DIALECT (ASME and ISO term pairs).

## Open questions

- Inflected languages (German compounds, Japanese without spaces) need morphology-aware matching or explicit inflection lists per entry [unverified: approach]. A leaf decides.
- Domain-expert reviewers per language need recruiting through OUTR community channels.

## Children

_None yet: status `open`._
