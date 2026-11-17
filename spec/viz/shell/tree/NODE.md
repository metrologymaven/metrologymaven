---
id: VIZ.SHELL.TREE
title: Element tree, inspector and results panels
level: 3
parent: VIZ.SHELL
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.SHELL-001, REQ-VIZ.SHELL-009]
covers: []
depends_on: [VIZ.SHELL.SPKTREE, VIZ.SHELL.SPKTABLE, VIZ.SHELL.DOCK, CORE]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.SHELL.TREE: Element tree, inspector and results panels

## Purpose

The dense data panels: the model/element tree (parts, features, characteristics, datums, alignments, results), the inspector property grid with schema-generated editors, the results table, the problems panel, the Python console host panel and the "Why?" provenance panel host. All edits go through registry commands.

## Scope

**In:**
- Element tree with virtualised rows, status chips, filters, drag and drop
- Inspector property grid generated from command and entity schemas, with explicit SI units
- Results table (filter by NONCONFORMS, INDETERMINATE, NOT_DECIDABLE; click to fly to)
- Problems panel (errors, warnings, stale results)
- Python console host panel (the console itself is AUTO.PY's)
- "Why?" panel host showing algorithm, kernel manifest, inputs, decision rule and replay action

**Out** (and which node owns it instead):
- Visibility state toggled from the tree (`VIZ.VIEW.VISIB`; the tree issues its commands)
- Python host process (`AUTO.PY`)
- Provenance data (`CORE`, `TRUST`)

## Requirements

- **REQ-VIZ.SHELL.TREE-001** Every edit made in the tree, inspector or results table shall be issued as a registry command.
- **REQ-VIZ.SHELL.TREE-002** When the element tree holds 10⁴ items, scrolling and expanding shall keep UI frame time under 33 ms on the reference profile [unverified].
- **REQ-VIZ.SHELL.TREE-003** The inspector shall generate editors from command schemas and shall show the SI unit of every numeric field.
- **REQ-VIZ.SHELL.TREE-004** Numbers in tables and the inspector shall be kernel-formatted for values of record and shall carry provenance badges.
- **REQ-VIZ.SHELL.TREE-005** Entity names and notes from imported content shall be displayed as untrusted plain text.

## Interfaces

- **Offers:** tree, inspector, results, problems, console-host and "Why?" panels through the DOCK panel host.
- **Consumes:** CORE entity tree and schemas; ResultRecords; status mapping from `VIZ.OVERLAY.STATUS` as data; `VIZ.SHELL.DOCK` panel host.

## Decomposition hints

ELTREE, INSPECT, RESULTS, PROBLEMS, CONSOLE, WHY.

## Open questions

- Blocked in practice until SPKTREE and SPKTABLE report; their findings set the widget approach.
- Applied: the roles and attributes requested by `FLOW.DEADEND.WHY` and `FLOW.RUNNER.CHKUI` are set by RESULTS, INSPECT, PROBLEMS and WHY using the vocabulary of `VIZ.SHELL.A11Y.AXTREE`; read-only provenance from `ECO.APPS.HANDOFF.RO` arrives through the `ReadOnlyInfo` trait in ELTREE (no code edge, since RO depends on this node).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| VIZ.SHELL.TREE.ELTREE | Virtualised element tree with status chips and read-only provenance (leaf) | 001 002 005 | none |
| VIZ.SHELL.TREE.INSPECT | Inspector property grid (leaf) | 001 003 004 005 | none |
| VIZ.SHELL.TREE.RESULTS | Results table with outcome filters and fly-to (leaf) | 001 004 005 | none |
| VIZ.SHELL.TREE.PROBLEMS | Problems panel with catalogued fix actions (leaf) | 001 005 | none |
| VIZ.SHELL.TREE.CONSOLE | Python console host panel (leaf, P1b) | 001 005 | none |
| VIZ.SHELL.TREE.WHY | Why? provenance panel host (leaf) | 004 005 | none |
