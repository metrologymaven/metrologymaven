---
id: I18N.CATALOG
title: String catalog, message IDs, plurals and hard-coded string lint
level: 2
parent: I18N
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-I18N-001]
covers: []
depends_on: [CORE.REG.DEF, RPT.MODEL.TMPL]
cli: [fid locale list-messages, fid locale show-message]
mcp: [locale_list_messages, locale_show_message]
journal: []
updated: 2026-11-16
---

# I18N.CATALOG: String catalog, message IDs, plurals and hard-coded string lint

## Purpose

The one source of user-facing text. Defines the message-ID scheme, the catalog file format (source language English, one file set per language), argument and plural handling, the lookup service with fallback, the extraction of strings from every surface (UI, CLI help, registry command titles and descriptions, typed error messages, report-template labels), and the CI lint that fails on hard-coded user-facing strings. Every other I18N child and every rendering surface reads text through this catalog, so it exists as its own unit.

## Scope

**In:**
- Message-ID scheme, including IDs derived from registry command IDs (`<command id>.title`, `<command id>.param.<name>`) and typed error codes (`<error code>.message`)
- Catalog file format with a schema version: message text, named arguments, plural categories per the Unicode CLDR plural rules [unverified: CLDR release to pin], select variants, translator comments, and a source digest per message
- Lookup API with fallback order: requested language, then the language's base (e.g. `zh-Hans` for `zh-Hans-CN`), then English; missing keys never crash
- Extraction tooling that harvests messages from UI code, CLI help, `CORE.REG.DEF` titles and descriptions, typed error declarations and report-template labels into the English source catalog
- Pseudo-localisation build (accented, expanded by about 40 %, bracketed) to find truncation and concatenation bugs [unverified expansion figure]
- CI lint: hard-coded user-facing strings, string concatenation of translated fragments, unknown or unused message IDs, argument mismatches between source and translation
- Locale-invariance boundary: journal payloads, MCP tool names and schemas, CLI `--json` output, error codes and file formats carry IDs and English source text only

**Out** (and which node owns it instead):
- Locale formatting of numbers and dates inside messages (`I18N.LOCALE`; CATALOG passes values through as typed arguments)
- Translating the messages and completeness gates (`I18N.XLATE`)
- Terminology consistency checks (`I18N.GLOSS`)
- Rendering scripts, fonts and IME (`I18N.SCRIPT`)
- Report-language selection and docs localisation (`I18N.CONTENT`)
- The UI-side lookup hook and message display (`VIZ.SHELL.THEME`, which consumes this catalog)
- Vocabulary and claim-word lint over the same strings (`ECO.RULES.VOCAB`)

## Requirements

- **REQ-I18N.CATALOG-001** Every user-facing string in UI, CLI help, registry command titles and descriptions, typed error messages and report-template labels shall be looked up by message ID from the string catalog.
- **REQ-I18N.CATALOG-002** If a source file in a product crate contains a user-facing string literal outside the catalog, then the CI lint shall fail with the file, line and a suggested message ID.
- **REQ-I18N.CATALOG-003** If a message assembles a sentence by concatenating two or more translated fragments, then the CI lint shall fail and name the message IDs involved.
- **REQ-I18N.CATALOG-004** The catalog format shall express plural forms by the CLDR plural categories of each language, and a message with a count argument shall select its variant by those categories.
- **REQ-I18N.CATALOG-005** If a message ID is missing in the requested language, then the lookup shall return the fallback-language text and record a `locale.missing_message` diagnostic, and shall never fail the calling command.
- **REQ-I18N.CATALOG-006** If a translation's named arguments differ from its source message's arguments, then the catalog check shall fail with the language, message ID and the differing argument names.
- **REQ-I18N.CATALOG-007** Every registry command shall have catalog entries for its title and each parameter label, and the build shall fail when a registered command lacks them.
- **REQ-I18N.CATALOG-008** Journal payloads, MCP tool names and schemas, CLI `--json` output and typed error codes shall be byte-identical under every UI language.
- **REQ-I18N.CATALOG-009** Each catalog message shall carry a digest of its English source text, so a translation made against an older source is detectable as stale.
- **REQ-I18N.CATALOG-010** The pseudo-localised build shall render every UI panel without a lookup error, and CI shall run it on every change to catalog or UI code.

## Interfaces

- **Offers:** catalog lookup service (`message(id, args, language)`), catalog files and their schema, extraction tool, CI lint, pseudo-locale; R0 commands `locale.list_messages@1` and `locale.show_message@1` (query messages by ID prefix, language and staleness).
- **Consumes:** command declarations (`CORE.REG.DEF` titles, parameter names, error declarations); template label slots (`RPT.MODEL.TMPL`).
- **Consumed by:** `VIZ.SHELL.THEME` (UI lookup hook), `RPT.MODEL` (labels), `OUTR.DOCS.REF` (translated command titles), `ECO.RULES.VOCAB` (lints the catalog files), all other I18N children.

## Decomposition hints

FORMAT (catalog schema, plurals, arguments, digests), LOOKUP (runtime API and fallback), EXTRACT (harvesters per surface), LINT (hard-coded strings, concatenation, argument checks), PSEUDO (pseudo-locale build).

## Open questions

- Catalog syntax: ICU MessageFormat 2 or Project Fluent [unverified: maturity of Rust implementations]. A leaf decides; both express CLDR plurals.
- `VIZ.SHELL.THEME` owns a "localisation catalogue hook" and `ECO.RULES.VOCAB` says the catalogue format belongs to `VIZ.SHELL` and `RPT`. This node takes the format and extraction; THEME keeps only the UI lookup hook. Those nodes' Out sections should be aligned in a later review.

## Children

_None yet: status `open`._
