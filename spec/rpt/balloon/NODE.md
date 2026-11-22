---
id: RPT.BALLOON
title: Drawing ballooning and manual readings
level: 2
parent: RPT
kind: subsystem
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT-006, REQ-RPT-002]
covers: []
depends_on: [RPT.MODEL, CORE, IO, VIZ]
cli: [fid report *-balloon, fid report *-balloons, fid report *-balloon-pack, fid report extract-drawing-text, fid report *-reading, fid report *-gauge]
mcp: [report_*_balloon, report_*_balloons, report_*_balloon_pack, report_extract_drawing_text, report_*_reading, report_*_gauge]
journal: [report.*_balloon, report.*_balloons, report.*_balloon_pack, report.extract_drawing_text, report.*_reading, report.*_gauge]
updated: 2026-11-22
---

# RPT.BALLOON: Drawing ballooning and manual readings

## Purpose

Lets a user load a 2D drawing (PDF or raster), place numbered balloons on its callouts, link each balloon to a characteristic, and type in readings taken with hand gauges. Each manual reading becomes a journaled ResultRecord with basis `DIRECT_READING` and the gauge it cites. This is how shops without CAD-based programs, or with mixed CMM and hand-gauge checks, get every characteristic into one report.

## Scope

**In:**
- Drawing sheets as project documents (multi-page PDF and raster images), stored by content digest
- Balloon register: number, sheet, position, linked characteristic, optional zone
- Creating characteristics from a balloon (nominal, tolerances, type) when no CAD/PMI exists
- Gauge register: gauge ID, type, resolution, calibration due date
- Manual reading entry, single and repeated (per part in a multi-part run)
- Renumbering with stable balloon IDs
- A ballooned-drawing block for reports

**Out** (and which node owns it instead):
- Reading PDF and image files (`IO`)
- Characteristic model and evaluation of the reading against tolerance (`GDT`)
- Decision rules and uncertainty of the reading (`UNC`)
- FAI form layout (`RPT.FORMS`)
- Automatic callout recognition (OCR) on drawings: not in scope here; see Open questions

## Requirements

- **REQ-RPT.BALLOON-001** When the user places a balloon, the system shall assign the next free balloon number and a stable ID, and journal the placement.
- **REQ-RPT.BALLOON-002** Each balloon shall link to exactly one characteristic, and each characteristic shall have at most one balloon per drawing revision.
- **REQ-RPT.BALLOON-003** When the user renumbers balloons, the system shall keep each balloon's stable ID and characteristic link unchanged.
- **REQ-RPT.BALLOON-004** When the user enters a manual reading, the system shall create a ResultRecord with evaluation basis `DIRECT_READING`, the entered value and unit, and the cited gauge ID.
- **REQ-RPT.BALLOON-005** If a manual reading cites no gauge, then the system shall reject it with error `report.reading_without_gauge`.
- **REQ-RPT.BALLOON-006** If the cited gauge's calibration due date has passed on the entry date, then the system shall record the reading with a `gauge_calibration_overdue` flag and warn the user.
- **REQ-RPT.BALLOON-007** If a manual reading has more decimal places than the cited gauge's resolution supports, then the system shall warn the user and keep the value as entered.
- **REQ-RPT.BALLOON-008** A manual reading shall carry uncertainty status UEC-0 unless the gauge register provides an uncertainty model accepted by UNC.
- **REQ-RPT.BALLOON-009** Balloon positions shall be stored in sheet coordinates so that they stay attached to the callout when the sheet is zoomed, panned or exported.
- **REQ-RPT.BALLOON-010** Typed reading values and drawing text shall be treated as untrusted content (CONSTITUTION C6).
- **REQ-RPT.BALLOON-011** The system shall support automated extraction of callout text and bounding boxes from 2D PDF drawings and batched balloon/characteristic proposals on a draft or agent branch for human H2 adoption, plus one-command export of a ballooned PDF and AS9102 Form 3 starter pack.

## Interfaces

- **Offers:** `report.add_balloon@1`, `report.move_balloon@1`, `report.renumber_balloon@1`, `report.enter_reading@1`, `report.void_reading@1`, `report.extract_drawing_text@1`, `report.propose_balloons@1`, `report.export_balloon_pack@1`, gauge register commands; types `DrawingSheet`, `Balloon`, `Gauge`, `ManualReading`, `BalloonProposal`; a ballooned-drawing block for `RPT.MODEL`.
- **Consumes:** PDF and image import (IO), characteristics and tolerance evaluation (GDT), decision rules (UNC), sheet viewer (VIZ), journal (CORE).
- **Side effects:** W2 (working document), W1 (auto-proposals on draft/agent branch and local export root). Agents may propose balloons on an agent branch; a human adopts them (CONSTITUTION C5). Agents shall not enter readings on behalf of an operator.

## Decomposition hints

SHEET (drawing documents), REG (balloon register and renumbering), CHAR (characteristic from balloon), GAUGE (gauge register), ENTRY (manual reading entry), BLOCK (ballooned-drawing report block), AUTO (automated 2D PDF extraction, balloon proposals and quick pack export).

## Split rationale

The split follows the **objects an operator handles**: drawings (SHEET), balloons on them (REG), characteristics created from balloons when there is no CAD (CHAR), the gauges readings cite (GAUGE), the readings themselves (ENTRY), the report block that shows the result (BLOCK), and automated 2D PDF callout extraction/proposal plus quick pack export (AUTO). Each owns its own commands, and the order matches the workflow: load drawing, auto-propose or manually place balloons, define, register gauges, enter readings, report.

- REG is finished as a leaf: its numbering rules are exact and FAI and PPAP row order depends on them.
- GAUGE is separate from ENTRY because the register outlives any one reading and may later be shared with `HAL.DRV` (see Open questions).
- Gauge commands (`*-gauge`) were added to this node's CLI, MCP and journal patterns.

Alternatives considered:
- **Drawing work vs. readings** (two children). Rejected: each would be XL and mix asset data (gauges) with results.
- **Gauge register in HAL.** Not chosen unilaterally: it is in this node's agreed scope; the overlap is raised as a cross-domain question.

## Open questions

- Resolved: automatic 2D PDF callout extraction, balloon proposals (with optional local/MCP vision-OCR candidates for scanned drawings) and quick ballooned-PDF + Form 3 export are now owned by `RPT.BALLOON.AUTO` (P1a, `research/pdf-ballooning/DESIGN.md`).
- The constraint "agents shall not enter readings" is a proposed reading of C5 and REQ-FID-021; confirm with maintainers (carried into `RPT.BALLOON.ENTRY`).
- No IO node names a PDF or raster page reader for drawings (resolved as `IO.REG.PLUGIN.DRAW`; PDF text and display tiles are owned by `RPT.BALLOON.AUTO.TEXT`).
- Gauge register overlap with `HAL.DRV` (raised in `RPT.BALLOON.GAUGE`).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| RPT.BALLOON.SHEET | Drawing sheets and drawing revisions | 009 010 | none |
| RPT.BALLOON.REG | Balloon register and renumbering (leaf) | 001 002 003 009 | none |
| RPT.BALLOON.CHAR | Characteristics from balloons | 002 | none |
| RPT.BALLOON.GAUGE | Gauge register | 006 007 008 | none |
| RPT.BALLOON.ENTRY | Manual reading entry | 004 005 006 007 008 010 | none |
| RPT.BALLOON.BLOCK | Ballooned-drawing report block | 009 | none |
| RPT.BALLOON.AUTO | Automated 2D PDF callout extraction, balloon proposals and quick pack export | 001 002 009 010 011 | none |
