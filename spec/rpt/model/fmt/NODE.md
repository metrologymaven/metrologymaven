---
id: RPT.MODEL.FMT
title: Value formatting, decision display and source labels
level: 3
parent: RPT.MODEL
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.MODEL-005, REQ-RPT.MODEL-006, REQ-RPT.MODEL-007, REQ-RPT.MODEL-008, REQ-RPT.MODEL-009]
covers: []
depends_on: [KRN.NUM.FMT.REG, KRN.NUM.FMT.LEN, KRN.NUM.FMT.ANG, KRN.NUM.KDEC, UNC.DEC.TYPE, UNC.UEC.CLASS, IO.RES.TRANS.MODEL, GDT.CHAR.RECORD, CORE.STO.SET]
uses_artifacts: [I18N.LOCALE, I18N.CATALOG]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# RPT.MODEL.FMT: Value formatting, decision display and source labels

## Purpose

The single place in RPT where a value of record becomes text. It chooses the kernel formatter, unit and precision for each value, calls `fmt::format` with the limits so the kernel's side-preserving rule applies, turns the kernel's `tie_at_max_precision` flag (or the angle formatter's `undecidable_side` result) into a visible marker, applies the locale's separators to the kernel's canonical string without re-rounding, and attaches decision, uncertainty and source labels. It holds no float-to-text code of its own. Every RPT node that shows a number goes through it.

## Scope

**In:**
- `ValueText` (canonical string, display string, unit, formatter ID and version, `digits_added`, markers, labels) and `OutcomeText`
- Formatter selection: kind by characteristic type (length, angle, scalar), display unit (authored unit unless the report overrides it), precision (authored decimals, else the per-unit default setting)
- Conformance-safe call: pass the limits the decision was made against, consume `digits_added`, `tie_at_max_precision` and `undecidable_side` from `KRN.NUM.FMT`
- Locale presentation: decimal separator, integer grouping and minus sign from the pinned locale data published by `I18N.LOCALE`, applied to the canonical string only
- Labels: decision outcome and rule ID; UEC display with UEC-0 as "not evaluated"; `transcribed` status with source software and version for `RESULTS_ONLY`; gauge ID for `DIRECT_READING`
- Fixed texts as catalog message keys with English fallback
- The CI rule that forbids float formatting in RPT crates

**Out:**
- The formatters, digit engine and exact comparator (`KRN.NUM.FMT.*`, `KRN.NUM.KDEC`)
- Decision rules and uncertainty classes (`UNC.DEC`, `UNC.UEC`)
- Transcription record content (`IO.RES.TRANS.MODEL`)
- Locale data and its golden corpus (`I18N.LOCALE`); message translations (`I18N.CATALOG`)
- Binding and resolved document assembly (`RPT.MODEL.BIND.GEN`)

## Requirements

- **REQ-RPT.MODEL.FMT-001** Every numeric value placed in a resolved report shall be the `Reported.value` of a `KRN.NUM.FMT` formatter call, and its `ValueText` shall record that formatter's ID and version.
- **REQ-RPT.MODEL.FMT-002** RPT crates shall contain no float-to-decimal conversion of their own, and a CI rule shall fail the build on any use of `format!`/`Display`/`to_string` on `f32` or `f64` in them.
- **REQ-RPT.MODEL.FMT-003** When formatting a value whose decision is `NONCONFORMS`, the system shall pass to the formatter the limits the decision was evaluated against (the acceptance limits when the rule has guard bands, otherwise the specification limits), so the formatter adds digits until the text lies strictly on the violated side, and shall record `digits_added` in the `ValueText`.
- **REQ-RPT.MODEL.FMT-004** If the formatter returns `tie_at_max_precision = true` or an `undecidable_side` result for a `NONCONFORMS` value, then the system shall add the visible nonconformance marker to the value and record warning `report.display_tie` or `report.display_undecidable` with the record ID.
- **REQ-RPT.MODEL.FMT-005** When a value has evaluation basis `RESULTS_ONLY`, the system shall label it with record status `transcribed` and the source software name and version from its transcription record.
- **REQ-RPT.MODEL.FMT-006** When a value has evaluation basis `DIRECT_READING`, the system shall label it with the cited gauge ID.
- **REQ-RPT.MODEL.FMT-007** The system shall show each result's decision outcome and decision rule ID, and shall show uncertainty class UEC-0 as the catalog text for "not evaluated" and never as a number.
- **REQ-RPT.MODEL.FMT-008** When a value is shown in a unit other than SI, the unit conversion shall be the formatter's single exact conversion, and the `ValueText` shall carry the formatter's `exact_conversion` record.
- **REQ-RPT.MODEL.FMT-009** The precision shall be the characteristic's authored decimals when present, and otherwise the per-unit default from the user setting `report.default_decimals` (initial values mm 3, um 1, in 4, deg 4, rad 6, unit 1: 4 [unverified]).
- **REQ-RPT.MODEL.FMT-010** The display string shall differ from the canonical string only in decimal separator, integer-part grouping and minus sign as given by the pinned locale data, shall keep every digit, and for every pair of canonical strings in the `I18N.LOCALE` golden corpus that differ under `kdec`, the display strings shall differ.
- **REQ-RPT.MODEL.FMT-011** If a value is non-finite, or its characteristic type has no formatter kind, then formatting shall fail with error `report.unformattable` naming the record, and no text shall be produced.
- **REQ-RPT.MODEL.FMT-012** `fmt_value` shall be the only producer of the `value_text@1` documents stored in report block value fields (shape pinned in `RPT.MODEL.DOC` Design), and shall label each value `Claimed`, `DeclaredInput` and `UncharacterizedInput` exactly when its fid-kunc uncertainty statement's `is_claimed()`, `rests_on_declared_input()` and `rests_on_uncharacterized_input()` are true.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/model/fmt_cases` (records with authored decimals, without, in mm, in, deg and unit 1), when each is formatted with locale `en`, then the canonical strings equal the golden file `fmt_cases.golden.json` and each `ValueText` names its formatter ID and version.
- **AC-2** (side rule) Given a `NONCONFORMS` record with value the next f64 above 5e-5 m and upper limit 0.050 mm at 3 decimals, when formatted, then the text is not `0.050`, `digits_added >= 1`, and no marker is set.
- **AC-3** (tie) Given a stub formatter returning `tie_at_max_precision = true`, and separately an angle record for which `fmt.angle@1` returns `undecidable_side`, when formatted, then each value carries the marker and warnings `report.display_tie` and `report.display_undecidable` are recorded.
- **AC-4** (guard band) Given a `NONCONFORMS` record decided under a guard-banded rule with acceptance upper limit 0.048 mm and value 0.04800001 mm, when formatted at 3 decimals, then the text is strictly greater than `0.048`.
- **AC-5** (labels) Given a `RESULTS_ONLY` record from fixture `fixtures/io/res/trans/sample_dmis` and a `DIRECT_READING` record citing gauge `G-17`, then the first is labelled `transcribed` with source software name and version, the second with `G-17`; a UEC-0 record's uncertainty text is the "not evaluated" catalog text.
- **AC-6** (locale) Given locale `de` and the `I18N.LOCALE` golden corpus, when every canonical string is rendered, then only separators and minus sign change, digits and trailing zeros are kept, and no two distinct canonical strings render alike.
- **AC-7** (lint) Given a test crate under the RPT lint configuration with `format!("{}", 1.5_f64)`, when CI lint runs, then it fails.
- **AC-8** (error path) Given a NaN value, when formatted, then it fails with `report.unformattable` and produces no text.

## Design

- **Registry commands:** none; library API `fmt_value(record: &ResultRecord, ch: &Characteristic, ctx: &FmtContext) -> Result<ValueText, FmtError>`, `fmt_requirement(ch, ctx) -> ValueText`, `fmt_outcome(decision) -> OutcomeText`, `labels(record) -> Vec<Label>`.
- **Data structures:** `FmtContext { locale_rules: LocaleRules, unit_override: Option<Unit>, default_decimals: BTreeMap<Unit, u8>, catalog: &Catalog }`; `ValueText { canonical: String, display: String, unit, formatter: (FormatterId, Version), exact_conversion, digits_added: u8, markers: Vec<Marker>, labels: Vec<Label> }`; `Label { Transcribed { software, version } | Gauge(GaugeId) | Rule(RuleId) | Uec(UecClass) | Claimed | DeclaredInput | UncharacterizedInput }` (the last three map to `RPT.MODEL.DOC`'s `ValueLabel`; a `ValueText` serialises to DOC's `value_text@1`).
- **Algorithm:** choose kind (length, angle, scalar) from the characteristic type; unit = override or authored unit; precision = authored decimals or default; limits = acceptance limits from the decision record if present else spec limits (as exact `kdec::Decimal` in SI); call `fmt::format` (`KRN.NUM.FMT.REG`); map flags to markers and warnings; render display string by substituting separators and grouping from `LocaleRules` (no re-rounding); attach labels from `UNC.DEC.TYPE`, `UNC.UEC.CLASS` and `IO.RES.TRANS.MODEL` fields.
- **Locale data:** read as the pinned data file published by `I18N.LOCALE` (an L3 runtime may not be a code dependency of an L2 domain, CONSTITUTION §2.1); its release ID goes into the resolved report's provenance.
- **Errors:** `report.unformattable`; formatter errors are wrapped, never swallowed.
- **Determinism:** D0; integer and string operations only.
- **Performance budget:** 100 000 values in under 500 ms on reference hardware [unverified until measured].
- **Crate:** `fid-report` (module `fmt`); lint configuration in the workspace for all `fid-report*` crates.

## Test fixtures

`fixtures/rpt/model/fmt_cases` and its golden JSON (synthetic, generated by an offline exact-rational script, CC-BY-4.0); `fixtures/io/res/trans/sample_dmis` (from `IO.RES.TRANS`); the `I18N.LOCALE` golden canonical-string corpus (artifact).

## Build prompt

> In crate `fid-report`, add module `fmt`, the only place where RPT turns values of record into text.
>
> **API:** `fmt_value`, `fmt_requirement`, `fmt_outcome`, `labels`, returning `ValueText { canonical, display, unit, formatter (id, version), exact_conversion, digits_added, markers, labels }`.
>
> **Rules:** call the kernel formatters through `fid_num::fmt::format` (`fmt.length@1`, `fmt.scalar@1`, `fmt.angle@1`) and never format floats yourself (add a workspace lint for `fid-report*` crates that rejects float `Display`/`format!`). Unit = authored unit or report override; precision = authored decimals or the user setting `report.default_decimals` (mm 3, um 1, in 4, deg 4, rad 6, unit 4). For `NONCONFORMS`, pass the limits the decision used (acceptance limits for guard-banded rules, else spec limits) so the kernel's side-preserving rule adds digits; record `digits_added`. `tie_at_max_precision` or `undecidable_side` -> visible marker plus warning `report.display_tie` / `report.display_undecidable`. Display string = canonical string with the pinned locale data's decimal separator, integer grouping and minus sign; never re-round; must stay injective over the I18N golden corpus. Labels: outcome and rule ID, UEC with UEC-0 as the catalog "not evaluated" text, `transcribed` + software name/version for `RESULTS_ONLY`, gauge ID for `DIRECT_READING`. Non-finite or unsupported type -> `report.unformattable`.
>
> **Acceptance:** AC-1..AC-8 in `spec/rpt/model/fmt/NODE.md`.
>
> **Constraints:** CONSTITUTION C1 (no value or decision computed here), C6, §4 (one journaled exact conversion), §8 (no panic, no `unsafe`, deny `unwrap`/`expect`); no std HashMap/HashSet; do not link I18N runtime crates (read their published data files).

## Open questions

- **Revise for D-035 (accepted 2026-11-17):** call the L0 I18N locale render transform directly instead of re-applying I18N's rules.
- **Applied (KRN.NUM.FMT.LEN note):** the parent requirement REQ-RPT.MODEL.FMT-003 described RPT requesting one digit at a time. The kernel formatter already does this when given limits, so this leaf passes the limits and consumes `digits_added` and `tie_at_max_precision` instead of looping; the parent's intent is unchanged.
- **Confirmed for RPT (KRN.NUM.FMT.LEN open question):** a conforming value that rounds to its limit may print as the limit; REQ-RPT-007 forbids only the nonconforming case. `UNC.DEC` should still confirm before `fmt.length@1` is frozen.
- Guard-banded rules: this leaf assumes the decision record carries the acceptance limits it used [unverified against `UNC.DEC.EVAL`]; if not, UNC must add them or RPT can only pass spec limits.
- Cross-domain: `I18N.LOCALE` owns the render transform but is an L3 runtime, so RPT re-applies the same rules from the published data file and proves equivalence with I18N's golden corpus. Moving the pure render transform into an L0 library would remove the duplication; raise at the `I18N.LOCALE` expansion.
- The `report.default_decimals` initial values are [unverified]; a maintainer may change them as data.

## Children

_Leaf: none._
