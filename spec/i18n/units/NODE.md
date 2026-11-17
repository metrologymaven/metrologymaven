---
id: I18N.UNITS
title: Display-unit preferences and display-unit input
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
depends_on: [GEO.FRAME.UNITS, KRN.NUM.FMT, CORE.STO.SET, CORE.BUS.UNIT]
cli: [fid locale list-display-units, fid locale show-display-units]
mcp: [locale_list_display_units, locale_show_display_units]
journal: []
updated: 2026-11-16
---

# I18N.UNITS: Display-unit preferences and display-unit input

## Purpose

Lets each user see and type lengths and angles in the units of their shop (mm or inch, micrometre or thou, decimal degrees or degrees-minutes-seconds) while every stored value stays SI (CONSTITUTION §4). It owns the preference model, the default precision mapping between unit systems and the rule that values typed in a display unit become authored quantities in that unit. The conversion arithmetic belongs to `GEO.FRAME.UNITS` and the kernel formatters; this node only chooses which unit and precision to ask them for.

## Scope

**In:**
- User-scope display-unit settings for length, small length (deviations, form values), angle, and angle style (decimal or DMS)
- Unit systems as presets: metric (mm, µm, deg), inch (in, thou, deg), plus per-kind overrides
- Default precision mapping when the system changes (for example mm with 3 decimals to inch with 4) [unverified convention]
- Display-unit input: a value typed in the UI is an `AuthoredQuantity` in the display unit, keeping its text, normalised by `CORE.BUS.UNIT`
- Unit labels and symbols (from the catalog) shown with every displayed value
- R0 commands listing the available display units and the user's resolved choices

**Out** (and which node owns it instead):
- Unit catalogue and exact conversion factors (`GEO.FRAME.UNITS`, `CORE.REG.DEF`)
- Producing the digits and the side-preserving rule (`KRN.NUM.FMT`)
- Per-report or per-characteristic unit choice inside a report (`RPT.MODEL.FMT`; this node supplies the default)
- Decimal separators and grouping (`I18N.LOCALE`)

## Requirements

- **REQ-I18N.UNITS-001** Display units shall be user-scope settings declared through `CORE.STO.SET`, with separate settings for length, small length, angle and angle style.
- **REQ-I18N.UNITS-002** Changing any display-unit setting shall change no stored value, journal entry, result digest or conformance decision, verified by regenerating a fixture project's results under both unit systems and comparing digests.
- **REQ-I18N.UNITS-003** Every displayed length or angle shall be produced by a `KRN.NUM.FMT` formatter in the display unit, so that inch, thou and DMS displays inherit the side-preserving rule.
- **REQ-I18N.UNITS-004** Inch and thou conversions shall use the exact factors 1 in = 25.4 mm and 1 thou = 0.0254 mm, with one rounding per displayed value.
- **REQ-I18N.UNITS-005** When a user switches unit system without an explicit precision override, the display precision shall follow the documented default mapping table, and the table shall be data covered by a test.
- **REQ-I18N.UNITS-006** When a user types a value in a display unit, the system shall store it as an authored quantity with its original text and unit, and shall not round-trip it through another unit before storage.
- **REQ-I18N.UNITS-007** Every displayed value shall show its unit symbol, and no screen or report shall show a bare number for a length or angle.

## Interfaces

- **Offers:** settings `units.length`, `units.small_length`, `units.angle`, `units.angle_style`, the preset table and precision mapping as data; R0 `locale.list_display_units@1` and `locale.show_display_units@1`.
- **Consumes:** unit catalogue and conversion (`GEO.FRAME.UNITS`); formatters (`KRN.NUM.FMT`); settings API (`CORE.STO.SET`); literal normalisation (`CORE.BUS.UNIT`).
- **Consumed by:** `RPT.MODEL.FMT` (default display units), `VIZ` readouts and tables, `I18N.CONTENT`.

## Decomposition hints

PREF (settings and presets), PREC (precision mapping data and tests), INPUT (display-unit input to authored quantity), INVAR (decision-invariance test harness across unit systems).

## Open questions

- Whether a report should, by default, show the limits in their authored unit when that differs from the display unit (dual-unit columns are common on inch drawings [unverified]). Owned jointly with `RPT.MODEL.FMT`.

## Children

_None yet: status `open`._
