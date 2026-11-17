---
id: RPT.MODEL.FMT
title: Value formatting, decision display and source labels
level: 3
parent: RPT.MODEL
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.MODEL-005, REQ-RPT.MODEL-006, REQ-RPT.MODEL-007, REQ-RPT.MODEL-008, REQ-RPT.MODEL-009]
covers: []
depends_on: [KRN.NUM, UNC.UEC, UNC.DEC, IO.RES]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# RPT.MODEL.FMT: Value formatting, decision display and source labels

## Purpose

The single place in RPT where a value of record becomes text. It calls the kernel's versioned number formatters (`fmt.*`) and exact decimal comparator (`kdec`), both owned by `KRN.NUM`, applies the conformance-safe display rule, renders decision outcomes and uncertainty status, and attaches source labels to transcription and direct-reading records. It holds no float-to-text code of its own. Every other RPT node that shows a number goes through it.

## Scope

**In:**
- Formatter selection per value (characteristic type, unit, authored decimals) and recording the formatter ID and version
- Conformance-safe display: extra digits for a nonconforming value that would print equal to its limit
- Decision outcome and decision-rule display; UEC display, with UEC-0 shown as "not evaluated"
- Source labels: source software and version for `RESULTS_ONLY` transcription records; gauge ID for `DIRECT_READING` records
- Display-unit conversion through the formatter (one journaled rounding, CONSTITUTION §4)

**Out** (and which node owns it instead):
- The formatters and the exact comparator themselves (`KRN.NUM`)
- Decision rules and uncertainty evaluation (`UNC.DEC`, `UNC.UEC`)
- Transcription record content (`IO.RES`)
- Binding and resolved document assembly (`RPT.MODEL.BIND`)

## Requirements

- **REQ-RPT.MODEL.FMT-001** Every numeric value placed in a resolved report shall be the output of a `KRN.NUM` `fmt.*` formatter, and the resolved report shall record that formatter's ID and version for the value.
- **REQ-RPT.MODEL.FMT-002** RPT crates shall contain no float-to-decimal conversion of their own, and a CI check shall fail on any use of standard float formatting in them.
- **REQ-RPT.MODEL.FMT-003** If a value's decision is `NONCONFORMS` and its formatted text equals the violated limit under the `kdec` exact comparator, then the system shall request one more digit at a time up to the formatter's maximum precision until the texts differ.
- **REQ-RPT.MODEL.FMT-004** If the texts are still equal at the formatter's maximum precision, then the system shall add a visible nonconformance marker to the value and record warning `report.display_tie`.
- **REQ-RPT.MODEL.FMT-005** When a value has evaluation basis `RESULTS_ONLY`, the system shall label it with record status `transcribed` and the source software name and version from its transcription record.
- **REQ-RPT.MODEL.FMT-006** When a value has evaluation basis `DIRECT_READING`, the system shall label it with the cited gauge ID.
- **REQ-RPT.MODEL.FMT-007** The system shall show each result's decision outcome and decision rule ID, and shall show uncertainty class UEC-0 as the fixed text "not evaluated" and never as a number.
- **REQ-RPT.MODEL.FMT-008** When a value is shown in a unit other than SI, the conversion shall be done by the formatter as a single rounding whose formatter ID is recorded.

## Interfaces

- **Offers:** a `ValueText` type (text, formatter ID and version, labels, markers) and a formatting service used by `RPT.MODEL.BIND`, `RPT.STATS` dashboards and `RPT.FORMS`.
- **Consumes:** `fmt.*@version` and `kdec` (`KRN.NUM`); decision outcomes (`UNC.DEC`); uncertainty classes (`UNC.UEC`); transcription records (`IO.RES`).

## Decomposition hints

SELECT (formatter selection and recording), SAFE (conformance-safe display and tie marker), LABEL (decision, UEC and source labels).

## Open questions

- Formatter selection policy (how many decimals for a characteristic with no authored decimals) may need a maintainer default; the formatter API itself waits on the `KRN.NUM` expansion.
- Localisation of fixed texts such as "not evaluated" (shared i18n catalog in VIZ.SHELL?) is not assigned.

## Children

_None yet: status `open`._
