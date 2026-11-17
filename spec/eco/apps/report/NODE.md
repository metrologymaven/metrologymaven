---
id: ECO.APPS.REPORT
title: Report app composition (reporting and SPC)
level: 3
parent: ECO.APPS
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO.APPS-002, REQ-ECO.APPS-006]
covers: []
depends_on: [ECO.APPS.MANIFEST]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.APPS.REPORT: Report app composition

## Purpose

Defines the Report app (D-004): the focused app for transcription and inspection reports, result review and multi-part statistics (trend, SPC, capability). It is the first app to ship because the P1a product (transcription reports, prismatic GD&T subset, minimal GUI) is a reporting workflow. This node owns the app's manifest content and its app-level defaults, not any reporting logic.

## Scope

**In:**
- Report app manifest: enabled namespaces (project, import of results and QIF, manual and gauge entry, characteristic evaluation, report, SPC once available, verification export)
- Default workspace layout, default report templates and default settings for reporting
- The app's declared status and known gaps
- Acceptance test: a P1a transcription-report workflow runs end to end in the app

**Out** (and which node owns it instead):
- Report designer, templates' engine and SPC computations (owned by `RPT`)
- GD&T evaluation (owned by `GDT`)
- Manifest schema (owned by `ECO.APPS.MANIFEST`)
- Other apps (owned by `ECO.APPS.INSPECT`, `ECO.APPS.PROBE`, `ECO.APPS.NETWORK`)

## Requirements

- **REQ-ECO.APPS.REPORT-001** The Report manifest shall enable no command whose side-effect class is D1 or M1.
- **REQ-ECO.APPS.REPORT-002** The Report manifest shall enable every namespace needed to run the P1a transcription-report workflow (create project, enter or import results, evaluate characteristics, generate and export a report, export a Result Package) and no namespace beyond the app's job.
- **REQ-ECO.APPS.REPORT-003** When a reference P1a transcription-report project is run in the Report app, the exported report and Result Package shall match those produced by the same journal replayed through the `fid` CLI.
- **REQ-ECO.APPS.REPORT-004** The Report manifest shall declare its status and list as known gaps every in-scope reporting seed row that is not yet live.

## Interfaces

- **Offers:** the Report app manifest and default templates selection.
- **Consumes:** ECO.APPS.MANIFEST schema; RPT, GDT, IO, TRUST and CORE namespaces through the registry.

## Decomposition hints

MAN (manifest content), DEFAULTS (layout, templates, settings), E2E (end-to-end P1a workflow test).

## Open questions

- Phase P1a is proposed because the P1a theme is a reporting workflow [unverified against the baseline's phase plan]; maintainers confirm in the PR.
- Whether SPC (P1b) ships inside the Report app or later as an option of it is an app-owner choice.

## Children

_None yet: status `open`._
