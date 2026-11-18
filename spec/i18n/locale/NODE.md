---
id: I18N.LOCALE
title: Locale data, display formatting and locale-aware input parsing
level: 2
parent: I18N
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-I18N-002, REQ-I18N-003]
covers: []
depends_on: [KRN.NUM.FMT, KRN.NUM.KDEC, CORE.STO.SET]
cli: [fid locale show-preferences, fid locale format-value, fid locale list-locales]
mcp: [locale_show_preferences, locale_format_value, locale_list_locales]
journal: []
updated: 2026-11-16
---

# I18N.LOCALE: Locale data, display formatting and locale-aware input parsing

## Purpose

Supplies the locale rules (decimal separator, grouping, minus sign, date, time and time-zone patterns) and two pure transforms built on them: a **render** transform that turns a kernel formatter's canonical decimal string into its locale form without re-rounding, and a **parse** transform that turns locale-formatted user input back into a canonical decimal string. The kernel formatters (`KRN.NUM.FMT`) decide the digits; this node only changes their presentation, so a locale can never change a value, a digit count or a conformance decision. It is separate from unit display (`I18N.UNITS`) because units change the number, while locale changes only the characters around it.

## Scope

**In:**
- Locale identifiers (BCP 47), the shipped locale list and the resolution order (explicit preference, OS locale, `en`)
- Locale data from Unicode CLDR at a pinned release, shipped as versioned data [unverified: CLDR licence entry in `ECO.RULES.SPDX`]
- Per-user locale preference declared as a user-scope setting through `CORE.STO.SET`
- Render transform for canonical decimal strings: decimal separator, integer-part grouping, minus sign, digits 0-9
- Parse transform for numeric input: locale decimal separator, grouping, full-width digits and signs (U+FF10-U+FF19, U+FF0B, U+FF0D, U+FF0E) normalised to ASCII
- Date, time and time-zone display for UI and reports
- `locale.format_value@1` (R1): show a value under a locale, for tests and support

**Out** (and which node owns it instead):
- Choosing digits, rounding and side-preserving output (`KRN.NUM.FMT`)
- Which formatter and decimals a report uses, and the tie marker (`RPT.MODEL.FMT`)
- Display units and unit conversion (`I18N.UNITS`)
- Message text and plural selection (`I18N.CATALOG`)
- Font coverage and IME composition (`I18N.SCRIPT`)

## Requirements

- **REQ-I18N.LOCALE-001** The render transform shall change only the decimal separator, the grouping separators in the integer part and the minus sign of a canonical decimal string, and shall preserve every digit, including trailing zeros.
- **REQ-I18N.LOCALE-002** For every shipped locale and every canonical decimal string in the test corpus, parsing the rendered string with the same locale shall return a string byte-identical to the canonical input.
- **REQ-I18N.LOCALE-003** If two canonical decimal strings differ under the `kdec` exact comparator, then their rendered forms in any shipped locale shall differ, so a nonconforming value is never displayed equal to its limit.
- **REQ-I18N.LOCALE-004** The render transform shall use the digits 0-9 for values of record in every shipped locale.
- **REQ-I18N.LOCALE-005** If numeric input is ambiguous under the active locale (for example `1,234` where the comma is the decimal separator and grouping is also accepted), then the parser shall reject it with error `locale.ambiguous_number` and shall not guess.
- **REQ-I18N.LOCALE-006** When numeric input contains full-width digits, signs or decimal points, the parser shall normalise them to ASCII before parsing.
- **REQ-I18N.LOCALE-007** The locale preference shall be a user-scope setting, and changing it shall change no journal entry, stored value, result digest or conformance decision.
- **REQ-I18N.LOCALE-008** A date printed in a report shall be either ISO 8601 (`YYYY-MM-DD`) or a locale form with a spelled-out or abbreviated month name, and shall never be a numeric form ambiguous between day-month and month-day order.
- **REQ-I18N.LOCALE-009** A time printed in a report shall show its UTC offset.
- **REQ-I18N.LOCALE-010** The locale data release shall be recorded with every rendered report, and updating it shall be a versioned change that a golden-string test detects.

## Interfaces

- **Offers:** `LocaleRules` data, `render(canonical, locale)`, `parse(input, locale)`, date and time formatting; the user-scope setting `locale.language`; R0 `locale.show_preferences@1` and `locale.list_locales@1`; R1 `locale.format_value@1`.
- **Consumes:** canonical strings and `Reported` records (`KRN.NUM.FMT`); exact comparator (`KRN.NUM.KDEC`); setting declaration API (`CORE.STO.SET`).
- **Consumed by:** `RPT.MODEL.FMT` (applies render to report values), `VIZ.SHELL.THEME` (UI-only numbers), `I18N.UNITS`, `I18N.CONTENT`.

## Decomposition hints

DATA (CLDR import, pinned release, locale list), RENDER (number render and the injectivity property test), PARSE (input parsing, full-width normalisation, ambiguity errors), DATE (date, time and zone), PREF (setting declaration and preference commands).

## Open questions

- **Revise for D-035 (accepted 2026-11-17):** the locale render transform is an L0 pure library so that RPT (L2) can call it directly instead of re-implementing it.
- `KRN.NUM.FMT` lists "locale-specific separators and typography" as owned by RPT and VIZ, while the parent I18N says I18N supplies locale rules. This node assumes: I18N.LOCALE supplies the rules and transforms; RPT and VIZ apply them. The KRN.NUM.FMT Out line should name `I18N.LOCALE`.
- Whether to group digits in the fractional part (some style guides group by three after the decimal point) [unverified]. Default: no fractional grouping.

## Children

_None yet: status `open`._
