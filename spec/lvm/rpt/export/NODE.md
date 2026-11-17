---
id: LVM.RPT.EXPORT
title: Network report assembly, RPT sections and CSV export
level: 3
parent: LVM.RPT
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-LVM.RPT-004]
covers: []
depends_on: [LVM.RPT.TABLES, LVM.RPT.SUMMARY, LVM.RPT.COMPARE, RPT.MODEL, IO.RES]
cli: [fid net report-network, fid net export-network-report]
mcp: [net_report_network, net_export_network_report]
journal: [net.export_network_report@1]
updated: 2026-11-16
---

# LVM.RPT.EXPORT: Network report assembly, RPT sections and CSV export

## Purpose

Assembles the summary, tables and optional comparison into one network report, publishes it as content blocks for the RPT template system, and exports the tables as CSV through IO. It owns the guarantee that regenerating a report from the same journal gives identical content.

## Scope

**In:**
- `net.report_network@1` assembling all blocks for one adjustment result
- Network report content blocks for RPT templates
- CSV export of tables into an approved local export root
- Byte-identical regeneration from the same journal

**Out** (and which node owns it instead):
- Table, summary and comparison content (`LVM.RPT.TABLES`, `LVM.RPT.SUMMARY`, `LVM.RPT.COMPARE`)
- Template engine, layout and PDF output (`RPT.MODEL`, `RPT.OUTPUT`)
- Results file formats (`IO.RES`)

## Requirements

- **REQ-LVM.RPT.EXPORT-001** When `net.report_network@1` runs, the system shall assemble the summary block, the residual tables, the scale-check table and, if requested, a comparison into one report content object.
- **REQ-LVM.RPT.EXPORT-002** Regenerating a network report from the same journal shall give byte-identical canonical content (RFC 8785 JSON) on every platform.
- **REQ-LVM.RPT.EXPORT-003** The system shall publish the network report as content blocks that RPT templates can place, without layout logic in this node.
- **REQ-LVM.RPT.EXPORT-004** When `net.export_network_report@1` writes CSV into an approved local export root, the command shall be W1 with `fs_effect: write_root` (CONSTITUTION §3.1), and agents shall pass only export-root handles.
- **REQ-LVM.RPT.EXPORT-005** Number formatting in exports shall use the unit and decimals stated in the report settings, and the full-precision values shall remain in the canonical content.

## Interfaces

- **Offers:** `net.report_network@1` (R1), `net.export_network_report@1` (W1, `fs_effect: write_root`); network report blocks for RPT.
- **Consumes:** `LVM.RPT.TABLES`, `LVM.RPT.SUMMARY`, `LVM.RPT.COMPARE`; `RPT.MODEL` block contract; `IO.RES` CSV writer.

## Decomposition hints

ASSEMBLE (report object and canonical form), BLOCKS (RPT content blocks), CSV (export).

## Open questions

- Should the CSV columns follow a results layout already defined by `IO.RES`, or a network-specific layout? Needs IO input.

## Children

_None yet: status `open`._
