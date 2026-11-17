---
id: RPT.BALLOON
title: Drawing ballooning and manual readings
level: 2
parent: RPT
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT-006, REQ-RPT-002]
covers: []
depends_on: [RPT.MODEL, CORE, IO, VIZ]
cli: [fid report *-balloon, fid report *-reading]
mcp: [report_*_balloon, report_*_reading]
journal: [report.*_balloon, report.*_reading]
updated: 2026-11-16
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

## Interfaces

- **Offers:** `report.add_balloon@1`, `report.move_balloon@1`, `report.renumber_balloon@1`, `report.enter_reading@1`, `report.void_reading@1`, gauge register commands; types `DrawingSheet`, `Balloon`, `Gauge`, `ManualReading`; a ballooned-drawing block for `RPT.MODEL`.
- **Consumes:** PDF and image import (IO), characteristics and tolerance evaluation (GDT), decision rules (UNC), sheet viewer (VIZ), journal (CORE).
- **Side effects:** W2. Agents may propose balloons on an agent branch; a human adopts them (CONSTITUTION C5). Agents shall not enter readings on behalf of an operator.

## Decomposition hints

SHEET (drawing documents), REG (balloon register and renumbering), CHAR (characteristic from balloon), GAUGE (gauge register), ENTRY (manual reading entry), BLOCK (ballooned-drawing report block).

## Open questions

- Automatic balloon proposal from PDF callouts or 3D PMI is a common incumbent-adjacent feature [unverified]. Not assigned to any node yet; candidate for a later child or for IND.
- The constraint "agents shall not enter readings" is a proposed reading of C5 and REQ-FID-021; confirm with maintainers.

## Children

_None yet: status `open`._
