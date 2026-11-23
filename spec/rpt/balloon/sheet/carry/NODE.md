---
id: RPT.BALLOON.SHEET.CARRY
title: Carry balloons forward to a new drawing revision
level: 4
parent: RPT.BALLOON.SHEET
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-RPT.BALLOON.SHEET-006]
covers: []
depends_on: [RPT.BALLOON.SHEET.IMPORT, CORE.BUS.PIPE]
side_effect: W2
determinism: D0
cli: [fid report carry-drawing-balloon]
mcp: [report_carry_drawing_balloon]
journal: [report.carry_drawing_balloon@1]
built_in: [fid-report]
updated: 2026-11-22
---

# RPT.BALLOON.SHEET.CARRY: Carry balloons forward to a new drawing revision

## Purpose

When a drawing is revised, most callouts stay where they were. This leaf copies the balloons of an earlier drawing revision onto a newly imported one: each carried balloon keeps its number, characteristic link and positions, gets a new balloon ID, and the new revision records which old balloon each one came from. Balloons are written through a `BalloonSink` trait that the registry wiring implements over the balloon register, so this leaf has no code dependency on `RPT.BALLOON.REG` (which depends on the sheet module) and no dependency cycle arises.

## Scope

**In:**
- `report.carry_drawing_balloon@1` (W2): from-revision, to-revision, sheet map (old sheet to new sheet, default by sheet index)
- The predecessor map in the target `DrawingRevision` (`carried_from`)
- The `BalloonSink` trait (`list(rev)`, `add_with_number(...)`) implemented at wiring over the register
- Reporting balloons that could not be carried (no mapped sheet, or off the new sheet's bounds)

**Out** (and which node owns it instead):
- Importing the new revision and sheet bounds (`RPT.BALLOON.SHEET.IMPORT`)
- Balloon numbering rules and storage (`RPT.BALLOON.REG`)
- Detecting moved callouts automatically: not in scope

## Requirements

- **REQ-RPT.BALLOON.SHEET.CARRY-001** When `report.carry_drawing_balloon@1` runs, the system shall create on the target revision one balloon per carried source balloon with the same number, characteristic link, anchor and label positions, and a new UUIDv7 ID, and journal one event.
- **REQ-RPT.BALLOON.SHEET.CARRY-002** The target revision shall record, for every carried balloon, the ID of its predecessor balloon in the source revision.
- **REQ-RPT.BALLOON.SHEET.CARRY-003** If a source balloon's sheet has no mapped target sheet, or its anchor or label lies outside the target sheet bounds, then the system shall not carry it and shall list it in the result as `not_carried` with the reason.
- **REQ-RPT.BALLOON.SHEET.CARRY-004** If the target revision already has balloons, belongs to a different part revision, or has a sheet that is not ready, then the command shall fail with error `report.carry_target_invalid` and journal nothing.
- **REQ-RPT.BALLOON.SHEET.CARRY-005** The source revision and its balloons shall remain unchanged.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/balloon/carry_two_revs` (revision B with two sheets and ten balloons; revision C imported with two sheets of the same size and no balloons), when `fid report carry-drawing-balloon --from B --to C --json` runs, then C has ten balloons with B's numbers, links and positions, new IDs, and a predecessor map of ten entries; B is unchanged.
- **AC-2** Given revision C whose sheet 2 is 10000 units narrower, then balloons on sheet 2 beyond the new width are listed `not_carried` with reason `off_sheet` and all others are carried.
- **AC-3** (error path) Given AC-1, when the carry runs again into C, then it fails with `report.carry_target_invalid` and the journal length is unchanged.
- **AC-4** (replay) Given the journal after AC-1, when replayed in exact mode, then the outcome is `IDENTICAL` and the new balloon IDs are unchanged.

## Design

- **Registry command:** `report.carry_drawing_balloon@1(from: DrawingRevId, to: DrawingRevId, sheet_map: Option<Vec<(SheetId, SheetId)>>) -> { carried: Vec<(BalloonId, BalloonId)>, not_carried: Vec<(BalloonId, Reason)> }`: W2, `--json`, `--dry-run`.
- **Trait:** `BalloonSink { fn list(&self, rev: DrawingRevId) -> Vec<BalloonView>; fn add_with_number(&mut self, rev, sheet, char, anchor, label, number, zone) -> Result<BalloonId, SinkError>; }`, implemented by the registry wiring over the balloon register in the same transaction.
- **Algorithm:** validate target → default sheet map by index → per source balloon in number order: map sheet, bounds check through `SheetInfo`, add through the sink → write the predecessor map → commit one event; any sink error rolls back all.
- **Errors:** `report.carry_target_invalid { reason }`, `report.not_found`, sink errors passed through.
- **Determinism:** D0; IDs journaled; processing in number order.
- **Performance budget:** carry 2000 balloons in under 50 ms [unverified until measured].
- **Crate:** `fid-report` (module `balloon::sheet::carry`).

## Test fixtures

- `fixtures/rpt/balloon/carry_two_revs`: two synthetic drawing revisions built by a test builder on blank sheets; CC-BY-4.0.

## Build prompt

> In crate `fid-report`, module `balloon::sheet::carry`, implement `report.carry_drawing_balloon@1` (W2, one journal event, `--json`, `--dry-run`; CLI `fid report carry-drawing-balloon`; MCP name derived).
>
> Copy every balloon of the source drawing revision to the target revision (same number, characteristic link, anchor and label; new UUIDv7 ID) through a `BalloonSink` trait that the registry wiring implements over the balloon register (do not depend on the register crate module directly). Map sheets by index unless a sheet map is given; skip and report as `not_carried` any balloon with no mapped sheet or off the target sheet bounds. Record the predecessor of each carried balloon in the target revision's `carried_from` map. Refuse targets that already have balloons, belong to another part revision, or have sheets not ready (`report.carry_target_invalid`). Leave the source untouched; any error rolls everything back and journals nothing.
>
> **Acceptance tests:** AC-1 to AC-4 (fixture `fixtures/rpt/balloon/carry_two_revs`), including exact replay giving `IDENTICAL`.
>
> **Constraints:** CONSTITUTION §8 (no panic, no `unsafe`, no `unwrap`/`expect`); integers only; `BTreeMap` only. Spec: `spec/rpt/balloon/sheet/carry/NODE.md`.

## Open questions

- `RPT.BALLOON.REG` depends on the whole `RPT.BALLOON.SHEET`; depending on `RPT.BALLOON.SHEET.IMPORT` only would let this leaf use the register directly instead of the injected sink.

## Children

_Leaf: none._
