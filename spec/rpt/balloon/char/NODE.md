---
id: RPT.BALLOON.CHAR
title: Characteristics from balloons
level: 3
parent: RPT.BALLOON
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.BALLOON-002]
covers: []
depends_on: [RPT.BALLOON.REG, GDT.CHAR.MODEL.CMDS, GDT.CHAR.SPEC, CORE.BUS.PIPE, CORE.BUS.UNIT]
side_effect: W2
determinism: D0
cli: [fid report add-char-balloon]
mcp: [report_add_char_balloon]
journal: [report.add_char_balloon@1]
updated: 2026-11-17
---

# RPT.BALLOON.CHAR: Characteristics from balloons

## Purpose

When a part has no CAD or PMI, the drawing is the only source of requirements. This leaf adds one composite command that creates a characteristic from what the user reads on the drawing (type, nominal, tolerances, unit, dialect) through the characteristic model owned by `GDT.CHAR`, and places its balloon in the register, as one atomic, journaled operation. It owns no characteristic rules: validation and identity stay in GDT.

## Scope

**In:**
- Command `report.add_char_balloon@1`: create the characteristic through `gdt.create_characteristic@1`'s handler and add the balloon through the `RPT.BALLOON.REG` register API in the same transaction
- Input of characteristic type, nominal, tolerances, unit and dialect as authored decimals with units
- The drawing-only type subset: size (linear and diameter), radius, distance, angle, and the form, orientation, location and runout callouts that `GDT.CHAR.MODEL.TYPES` defines
- Passing GDT validation errors through unchanged

**Out** (and which node owns it instead):
- Characteristic model, identity, validation, governing spec and evaluation (`GDT.CHAR.MODEL.CMDS`, `GDT.CHAR.IDENT`, `GDT.CHAR.SPEC`)
- Balloon numbering, positions and renumbering (`RPT.BALLOON.REG`)
- Attribute (pass/fail) characteristics from drawing notes: not in this leaf until GDT defines them (see Open questions)

## Requirements

- **REQ-RPT.BALLOON.CHAR-001** When `report.add_char_balloon@1` runs, the system shall create the characteristic and add its balloon in one transaction and journal exactly one `report.add_char_balloon@1` event that contains the full created characteristic and balloon.
- **REQ-RPT.BALLOON.CHAR-002** If the characteristic fails GDT validation, or the balloon fails a register rule, then neither the characteristic nor the balloon shall be created, nothing shall be journaled, and the GDT or register error shall be returned unchanged.
- **REQ-RPT.BALLOON.CHAR-003** Nominal and tolerance values shall be stored as authored decimals with their unit and converted to SI with one journaled rounding through `CORE.BUS.UNIT` (CONSTITUTION §4).
- **REQ-RPT.BALLOON.CHAR-004** If the dialect (ASME or ISO) is not given explicitly, then the command shall fail with error `report.dialect_missing` and journal nothing.
- **REQ-RPT.BALLOON.CHAR-005** If the requested characteristic type is not in the drawing-only type subset supported by `GDT.CHAR.MODEL.TYPES`, then the command shall fail with error `report.char_type_unsupported` naming the type.
- **REQ-RPT.BALLOON.CHAR-006** On exact replay, the characteristic ID and balloon ID shall be read back from the journal event, never generated anew.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/balloon/two_sheet_drawing` with an empty plan, when `fid report add-char-balloon --plan <p> --sheet <s1> --type diameter --nominal 12.000mm --upper +0.018mm --lower 0mm --dialect ASME --anchor 1000,2000 --label 1500,2000 --json` runs, then one characteristic and balloon 1 linked to it exist, and the journal gains exactly one `report.add_char_balloon@1` event.
- **AC-2** Given AC-1, when the characteristic is read with `fid gdt` commands, then its nominal is the authored literal `12.000 mm` and its tolerances are the authored literals.
- **AC-3** (error path) Given AC-1, when a characteristic is added whose lower limit exceeds the upper limit, then the GDT validation error is returned unchanged, no characteristic and no balloon are created and the journal length is unchanged.
- **AC-4** (error path) When `--dialect` is omitted, then it fails with `report.dialect_missing`; when `--number 1` is given again, then it fails with `report.balloon_number_taken` and no characteristic is created.
- **AC-5** (replay) Given the journal after AC-1, when replayed in exact mode, then the outcome is `IDENTICAL` and the characteristic and balloon IDs are unchanged.

## Design

- **Registry command:** `report.add_char_balloon@1(plan: PlanId, drawing_rev: DrawingRevId, sheet: SheetId, characteristic: DrawingCharInput, anchor: SheetPoint, label: SheetPoint, number: Option<u32>, zone: Option<Text>) -> { characteristic: Characteristic, balloon: Balloon }` (W2, `--json`, `--dry-run`).
- **Data structures:** `DrawingCharInput { kind: DrawingCharKind, nominal: AuthoredQuantity, upper: AuthoredQuantity, lower: AuthoredQuantity, dialect: Dialect, datum_refs: Vec<Untrusted<Text>>, name: Option<Untrusted<Text>> }`, mapped field by field onto GDT's `CharacteristicInput`.
- **Algorithm:** inside one bus transaction: (1) check dialect present and kind supported; (2) call the GDT create handler (validation, identity, governing spec inheritance); (3) call the register add function; (4) on any error roll back; (5) commit one event with both results.
- **Atomicity:** the composite handler runs inside the single commit of the `CORE.BUS.PIPE` execute pipeline, so a failure at any stage commits nothing.
- **Errors:** `report.dialect_missing`, `report.char_type_unsupported { kind }`, GDT validation errors (passed through), register errors (`report.balloon_number_taken`, `report.balloon_off_sheet`, `report.char_already_ballooned`).
- **Determinism:** D0; IDs are journaled values; unit conversion through `CORE.BUS.UNIT` with its versioned algorithm ID.
- **Performance budget:** under 5 ms per command excluding journal I/O [unverified until measured].
- **Crate:** `fid-report` (module `balloon::char`).

## Test fixtures

- `fixtures/rpt/balloon/two_sheet_drawing` (shared with `RPT.BALLOON.REG`) plus an empty plan created by the test builder; CC-BY-4.0.

## Build prompt

> In crate `fid-report`, module `balloon::char`, implement the composite command `report.add_char_balloon@1` (W2, `--json`, `--dry-run`; CLI `fid report add-char-balloon`, MCP name derived).
>
> It creates a characteristic by calling the handler of `gdt.create_characteristic@1` (GDT owns validation, identity and governing-spec inheritance) and then adds its balloon through the balloon-register API, in one bus transaction that commits exactly one journal event carrying both results. Any GDT or register error rolls back everything and is returned unchanged; nothing is journaled. Require an explicit dialect (`report.dialect_missing`), and accept only the drawing-only subset of characteristic kinds that GDT defines (`report.char_type_unsupported`). Keep nominal and tolerances as authored decimals with units and convert through the authored-literal normaliser (one journaled rounding). IDs are generated by the handlers and read back from the journal on replay.
>
> **Acceptance tests:** AC-1 to AC-5 (fixture `fixtures/rpt/balloon/two_sheet_drawing`), including exact replay giving `IDENTICAL`.
>
> **Constraints:** CONSTITUTION §8 (no panic, no `unsafe`, no `unwrap`/`expect`); C6 (names and datum labels are untrusted text); agents run this W2 command only on an agent branch, and a human adopts the result (C5). Do not reimplement characteristic validation. Spec: `spec/rpt/balloon/char/NODE.md`.

## Open questions

- **Cross-domain:** attribute (pass/fail) characteristics from drawing notes are not defined by `GDT.CHAR.MODEL.TYPES` yet; when GDT adds them, this leaf adds them to its subset in a minor version.

## Children

_Leaf: none._
