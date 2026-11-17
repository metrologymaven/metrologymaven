---
id: I18N
title: Internationalisation and localisation
level: 1
parent: FID
kind: domain
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-FID-029]
covers: []
depends_on: [VIZ, KRN, RPT, OUTR]
cli: [fid locale *]
mcp: [locale_*]
journal: [locale.*]
updated: 2026-11-16
---

# I18N: Internationalisation and localisation

## Purpose

Makes the product usable in its biggest markets: Germany, Japan, China, Korea, Mexico, Italy and France.
- **Internationalisation:** string extraction; plurals; right-to-left readiness; locale-aware dates and number display, separate from the kernel's exact formatting.
- **Units display:** inch or mm display while storage stays SI.
- **Localisation:** a community translation workflow, terminology glossaries per language for metrology terms, and localised reports, docs and tutorials.

## Scope

**In:**
- String catalog and extraction from UI, CLI help, command titles and report templates
- Locale data: dates, number grouping and decimal separators for display, with report values still formatted by kernel formatters under locale rules
- Unit display preferences (mm/inch, µm/thou), with exact conversion rules
- Translation workflow: community translation platform, review, completeness gates
- Metrology terminology glossaries per language, including GD&T symbol names and standard-specific terms
- Localised report templates and form variants
- Fonts and IME support for CJK
- Docs and tutorial localisation (with OUTR)

**Out:**
- Kernel number formatting itself (KRN.NUM; I18N supplies locale rules)
- Brand naming per market (OUTR.BRAND)

## Requirements

- **REQ-I18N-001** Every user-facing string in UI, CLI help, command titles, errors and report templates shall come from the string catalog, and CI shall fail on hard-coded user-facing strings.
- **REQ-I18N-002** Display units and locale formats shall be per-user preferences. Changing them shall never change a stored value or a conformance decision.
- **REQ-I18N-003** Report values shall be formatted by the kernel formatters with the locale's decimal separator and grouping, and shall never print a nonconforming value as equal to a limit in any locale.
- **REQ-I18N-004** The project shall run a community translation workflow with per-language reviewers and a completeness threshold before a language ships in a release.
- **REQ-I18N-005** The project shall maintain a reviewed metrology terminology glossary per shipped language, and translations shall be checked against it.
- **REQ-I18N-006** The UI shall render and accept input in CJK scripts, including IME composition, and in right-to-left scripts where shipped.
- **REQ-I18N-007** The first release beyond English shall include German, and the next set shall include Japanese and Simplified Chinese [unverified market order].

## Interfaces

- **Offers:** string catalog, locale and unit-display services, translation workflow, glossaries.
- **Consumes:** VIZ (UI text rendering), KRN.NUM (formatters), RPT (templates), OUTR (docs).

## Decomposition hints

CATALOG (string extraction and CI lint), LOCALE (locale data and display formatting), UNITS (unit display), XLATE (translation workflow), GLOSS (terminology glossaries), CJK (fonts, IME, RTL).

## Open questions

- Market order of languages needs design-partner and community input.

## Split rationale

Divided by **what changes and who owns it**: runtime text plumbing (CATALOG), presentation of numbers and dates that must never change a value (LOCALE), choice of display unit that changes the number but not the stored value (UNITS), the people process that produces translations (XLATE), the expert-reviewed reference data that constrains them (GLOSS), the script and input layer on the UI toolkit and PDF renderer (SCRIPT), and the long-form outputs that combine all of these (CONTENT). LOCALE and UNITS are kept apart because unit conversion goes through the kernel formatters and changes digits, while locale rendering is a pure, reversible character transform with its own injectivity test; both share REQ-I18N-002 and REQ-I18N-003.

Alternatives considered: one child per surface (UI, CLI, reports, docs) was rejected because each surface would re-implement catalog, locale and glossary rules; folding GLOSS into XLATE was rejected because glossary review needs domain experts and is also checked against docs and templates; a separate RTL child was rejected because no RTL language ships yet, so readiness fits inside SCRIPT.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| I18N.CATALOG | String catalog, message IDs, plurals and hard-coded string lint | REQ-I18N-001 | none |
| I18N.LOCALE | Locale data, display formatting and locale-aware input parsing | REQ-I18N-002, REQ-I18N-003 | none |
| I18N.UNITS | Display-unit preferences and display-unit input | REQ-I18N-002, REQ-I18N-003 | none |
| I18N.XLATE | Community translation workflow, review and release gate | REQ-I18N-004, REQ-I18N-007 | none |
| I18N.GLOSS | Metrology terminology glossaries and the glossary check | REQ-I18N-005 | none |
| I18N.SCRIPT | Scripts, fonts, IME composition and right-to-left readiness | REQ-I18N-006 | none |
| I18N.CONTENT | Localised reports, form variants, docs and tutorials | REQ-I18N-001, REQ-I18N-004 | none |
