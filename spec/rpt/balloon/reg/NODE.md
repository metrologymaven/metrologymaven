---
id: RPT.BALLOON.REG
title: Balloon register and renumbering
level: 3
parent: RPT.BALLOON
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-report]
built_pr: 587
satisfies: [REQ-RPT.BALLOON-001, REQ-RPT.BALLOON-002, REQ-RPT.BALLOON-003, REQ-RPT.BALLOON-009]
covers: []
depends_on: [RPT.BALLOON.SHEET, CORE.JRN, GDT.CHAR]
side_effect: W2
determinism: D0
cli: [fid report add-balloon, fid report move-balloon, fid report renumber-balloon, fid report relink-balloon, fid report remove-balloon, fid report list-balloon]
mcp: [report_add_balloon, report_move_balloon, report_renumber_balloon, report_relink_balloon, report_remove_balloon, report_list_balloon]
journal: [report.add_balloon@1, report.move_balloon@1, report.renumber_balloon@1, report.relink_balloon@1, report.remove_balloon@1]
updated: 2026-11-24
---

# RPT.BALLOON.REG: Balloon register and renumbering

## Purpose

The register of balloons on a drawing revision: each balloon has a stable ID, a number, a sheet, an anchor on the callout and a label position in sheet coordinates, an optional drawing zone, and a link to exactly one characteristic. This leaf implements placing, moving, renumbering, relinking, removing and listing balloons. FAI form 3 and PPAP rows (`RPT.FORMS`) are ordered by these numbers, so number rules must be exact.

## Scope

**In:** `Balloon` type; per-drawing-revision register; number assignment and uniqueness; one-balloon-per-characteristic rule; renumber modes (set, insert, compact) that keep IDs and links; positions in sheet coordinates with bounds check; the six commands above.

**Out:** drawing sheets and their coordinate system (`RPT.BALLOON.SHEET`); creating characteristics from a balloon (`RPT.BALLOON.CHAR`); manual readings (`RPT.BALLOON.ENTRY`); drawing the balloons on screen and in reports (`RPT.BALLOON.BLOCK`, VIZ).

## Requirements

- **REQ-RPT.BALLOON.REG-001** When `report.add_balloon@1` runs without an explicit number, the system shall assign one more than the highest balloon number in use on that drawing revision (1 if none) and a new UUIDv7 balloon ID, and journal one event.
- **REQ-RPT.BALLOON.REG-002** If `report.add_balloon@1` runs in mode `set` with an explicit number that is already in use on that drawing revision, then it shall fail with error `report.balloon_number_taken` and journal nothing.
- **REQ-RPT.BALLOON.REG-003** Each balloon shall link to exactly one characteristic, and if a characteristic already has a balloon on that drawing revision, then adding or relinking another shall fail with error `report.char_already_ballooned`.
- **REQ-RPT.BALLOON.REG-004** When `report.renumber_balloon@1` runs in mode `set`, the system shall give the balloon the target number if it is free, and otherwise fail with `report.balloon_number_taken`.
- **REQ-RPT.BALLOON.REG-005** When `report.renumber_balloon@1` runs in mode `insert`, the system shall give the balloon the target number and add one to every other balloon whose number is at or above the target, as one journaled event.
- **REQ-RPT.BALLOON.REG-006** When `report.renumber_balloon@1` runs in mode `compact`, the system shall renumber all balloons of the drawing revision to 1..n, preserving their current relative order.
- **REQ-RPT.BALLOON.REG-007** Every renumbering shall keep each balloon's ID and characteristic link unchanged.
- **REQ-RPT.BALLOON.REG-008** Balloon anchor and label positions shall be stored as integer sheet coordinates in 0.01 mm units on the sheet's coordinate system, and if a position lies outside the sheet bounds, then the command shall fail with error `report.balloon_off_sheet`.
- **REQ-RPT.BALLOON.REG-009** When a balloon is removed, its number shall become free and its ID shall never be reused.
- **REQ-RPT.BALLOON.REG-010** The optional zone field shall be untrusted text of at most 16 bytes (CONSTITUTION C6).
- **REQ-RPT.BALLOON.REG-011** `report.list_balloon@1` shall be R0 and return balloons ordered by ascending number.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/balloon/two_sheet_drawing` (one drawing revision, two A3 landscape sheets, ten characteristics C1–C10, no balloons), when ten `report.add_balloon@1` calls run with `characteristic: Ci`, `sheet: <s>`, `anchor: {x, y}` and `label: {x, y}` (`fid report add-balloon ... --json`), then the balloons are numbered 1–10 with distinct UUIDv7 IDs and the journal gains ten `report.add_balloon@1` events.
- **AC-2** (error path) Given AC-1, when a balloon is added for C3 again, then it fails with `report.char_already_ballooned` and the journal length is unchanged; when a balloon is added with `number: 4` (mode `set`), it fails with `report.balloon_number_taken`.
- **AC-3** Given AC-1, when balloon 7 is removed and balloon 9 is renumbered with `mode: set`, `number: 7`, then `list-balloon` shows numbers 1–6, 7 (old 9's ID), 8, 10 with every ID and link unchanged.
- **AC-4** Given AC-3, when a new balloon for C7 is added with `mode: insert`, `number: 2`, then it gets number 2, old numbers 2–10 shift up by one, and all prior IDs and links are unchanged.
- **AC-5** Given AC-4, when `report.renumber_balloon@1` runs with `mode: compact`, then numbers are 1..10 contiguous in the prior relative order.
- **AC-6** (error path) When a balloon is placed with `anchor: {x: 42001, y: 100}` on a sheet 42000 units wide, then it fails with `report.balloon_off_sheet`.
- **AC-7** (replay) Given the journal after AC-5, when replayed in exact mode, then the outcome is `IDENTICAL` and `list-balloon` output is byte-identical.

## Design

- **Registry commands** (W2 unless noted):
  - `report.add_balloon@1(drawing_rev: DrawingRevId, sheet: SheetId, characteristic: CharId, anchor: SheetPoint, label: SheetPoint, number: Option<u32>, mode: Set | Insert = Set, zone: Option<Text>) -> Balloon`; mode `insert` needs `number` and adds one to every balloon numbered at or above it in the same event (AC-4)
  - `report.move_balloon@1(balloon: BalloonId, anchor: Option<SheetPoint>, label: Option<SheetPoint>, sheet: Option<SheetId>) -> Balloon`
  - `report.renumber_balloon@1(drawing_rev: DrawingRevId, mode: Set{balloon, number} | Insert{balloon, number} | Compact) -> Vec<(BalloonId, old: u32, new: u32)>`
  - `report.relink_balloon@1(balloon: BalloonId, characteristic: CharId) -> Balloon`
  - `report.remove_balloon@1(balloon: BalloonId) -> ()`
  - `report.list_balloon@1(drawing_rev: DrawingRevId) -> Vec<Balloon>` (R0)
- **Data structures:** `Balloon { id: BalloonId (UUIDv7), number: u32 (>= 1), drawing_rev, sheet, characteristic: CharId, anchor: SheetPoint, label: SheetPoint, zone: Option<Untrusted<Text>> }`; `SheetPoint { x: i64, y: i64 }` in 0.01 mm from the sheet origin defined by `RPT.BALLOON.SHEET`. The register is a `BTreeMap<BalloonId, Balloon>` plus a `BTreeMap<u32, BalloonId>` number index and a `BTreeMap<CharId, BalloonId>` link index per drawing revision; also `retired_ids`.
- **Consumed interface:** a `SheetInfo` trait from `RPT.BALLOON.SHEET` giving `bounds(sheet) -> (width, height)` and `revision(sheet) -> DrawingRevId`. Until SHEET is built, tests use a stub implementation. Characteristics are looked up through a `CharacteristicIndex` trait (`contains(&CharId) -> bool`, `CharId` from `fid_gdt::chars::dprof`); an unknown characteristic is `report.not_found`. Until GDT.CHAR.MODEL.CMDS is built, tests stub it with a `BTreeSet`.
- **Algorithm:** insert mode shifts in descending number order inside one event so the number index never holds duplicates; compact mode sorts by current number.
- **Errors:** `report.balloon_number_taken`, `report.char_already_ballooned`, `report.balloon_off_sheet`, `report.sheet_not_ready` (the sheet's size is not known yet: `SheetInfo::bounds` is `None`, e.g. a raster sheet with status `needs_size`), `report.not_found`, `report.text_invalid`, `report.bad_params`. Checks run in requirement order: zone, number parameters, sheet, positions, characteristic, number in use (REQ-002), link (REQ-003).
- **Determinism:** D0; integers only; ordered maps.
- **Journal events.** Balloon events are not authority-bearing (no approval, sign-off or decision), so REQ-CORE.RPL.RECON.FOLD-008 does not apply; replay still takes the principal and entry from the journal entry, never the event body.
- **Replay environment.** Exact replay re-checks each event against the `SheetInfo` and `CharacteristicIndex` it is given, i.e. the current sheet bounds and characteristics, not those at the time of the event. Integration work must replay against the environment as of each entry (or snapshot it); otherwise a later sheet resize or characteristic removal makes an honest journal replay `DIVERGENT`.
- **Performance budget:** add, move or renumber in a register of 2000 balloons in under 10 ms on reference hardware [unverified until measured].
- **Crate:** `fid-report` (module `balloon::register`).

## Test fixtures

- `fixtures/rpt/balloon/two_sheet_drawing`: a synthetic drawing revision with two blank 420 × 297 mm sheets (no real drawing content) and ten stub characteristics; built by a test builder, authored in-project, CC-BY-4.0.

## Build prompt

> In crate `fid-report`, module `balloon::register`, implement the balloon register for drawing revisions.
>
> **Commands** (registry, `--json`, MCP names derived): `report.add_balloon@1`, `report.move_balloon@1`, `report.renumber_balloon@1` (modes `set`, `insert`, `compact`), `report.relink_balloon@1`, `report.remove_balloon@1` (all W2, one journal event each, failed commands journal nothing) and `report.list_balloon@1` (R0, ordered by number).
>
> **Rules:** IDs are UUIDv7, generated by the handler and read back from the journal on replay; never reuse an ID. Default number = highest in use + 1 (1 if none). Numbers unique per drawing revision (`report.balloon_number_taken`); at most one balloon per characteristic per drawing revision (`report.char_already_ballooned`). Renumbering never changes IDs or links; `insert` shifts numbers at or above the target up by one in a single event; `compact` makes 1..n in current order. Positions are `SheetPoint { x: i64, y: i64 }` in 0.01 mm; outside the sheet bounds -> `report.balloon_off_sheet`. Zone is untrusted text, at most 16 bytes. Use `BTreeMap`, never std `HashMap`. Sheet bounds come from a `SheetInfo` trait (stub it in tests).
>
> **Acceptance tests:** AC-1 to AC-7 above with fixture `fixtures/rpt/balloon/two_sheet_drawing`, including exact replay -> `IDENTICAL`.
>
> **Constraints:** CONSTITUTION §8 (no panic, no `unsafe`, no `unwrap`/`expect`); C6 untrusted text; agents use these W2 commands only on an agent branch, enforced by the command bus. Do not implement sheets, characteristic creation, readings or rendering. Spec: `spec/rpt/balloon/reg/NODE.md`.

## Open questions

- Should removing a balloon that already has readings be refused? Assumed allowed, because readings are linked to the characteristic, not the balloon; `RPT.FORMS` then reports the characteristic as unballooned.

## Children

_Leaf: none._
