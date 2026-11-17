---
id: ECO.APPS.HANDOFF
title: Cross-app project opening and hand-off
level: 3
parent: ECO.APPS
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO.APPS-004, REQ-ECO.APPS-005]
covers: []
depends_on: [ECO.APPS.MANIFEST, CORE.PRJ, CORE.JRN, VIZ.SHELL]
uses_artifacts: [ECO.REL.PKG.REG]
cli: [fid app list-editors]
mcp: [app_list_editors]
journal: [app.list_editors@1]
updated: 2026-11-17
---

# ECO.APPS.HANDOFF: Cross-app project opening and hand-off

## Purpose

Makes one project usable across the app family: any app opens, saves and journals the same project container; elements created by commands an app does not enable are shown read-only with provenance; and the user can hand the project to an app that enables them. It owns the behaviour at the app boundary, not the project format itself (CORE.PRJ).

## Scope

**In:**
- Cross-app round-trip guarantee and its test suite (open in app A, save, open in app B, compare)
- Read-only presentation rule for elements whose commands the current app does not enable
- "Open in <app>" hand-off: locating installed apps, launching on the same project, single-writer handling
- Listing which installed apps can edit a given element

**Out** (and which node owns it instead):
- Project container, stages and element tree (owned by `CORE.PRJ`)
- Journal format and branches (owned by `CORE.JRN`)
- Panel rendering of read-only elements (owned by `VIZ.SHELL`)
- Installed-app discovery on each OS as a packaging concern (owned by `ECO.REL.PKG`; this node reads its registration entries)

## Requirements

- **REQ-ECO.APPS.HANDOFF-001** When a project saved by one app is opened and saved again by another app without edits, the project's element IDs, journal entries and journal hash chain shall be unchanged.
- **REQ-ECO.APPS.HANDOFF-002** If the opening app does not enable the command namespace that created an element, then the app shall show the element read-only with its creating command ID, author principal and journal entry reference.
- **REQ-ECO.APPS.HANDOFF-003** If a user tries to modify an element whose namespace the app does not enable, then the app shall refuse the change without writing to the journal and shall list the installed apps that enable that namespace.
- **REQ-ECO.APPS.HANDOFF-004** When the user hands a project to another app, the handing app shall release its write lock before the receiving app opens the project, so that at most one app writes to a project at a time.
- **REQ-ECO.APPS.HANDOFF-005** The cross-app round-trip suite shall run in CI for every pair of apps that have a manifest.

## Interfaces

- **Offers:** `app.list_editors@1` (R0, in ECO.APPS.HANDOFF.EDITORS); the edit guard and read-only provenance data (ECO.APPS.HANDOFF.RO); the "Open in" UI action, which is not a registry command (CONSTITUTION §3.1, D-031 item 4; ECO.APPS.HANDOFF.OPENIN); the round-trip suite and its report (ECO.APPS.HANDOFF.RTRIP).
- **Consumes:** CORE.PRJ container and element tree; CORE.STO.LAYOUT write lock; CORE.JRN entries; ECO.APPS.MANIFEST loader and effective namespaces; VIZ.SHELL read-only presentation; installed-app registration entries published by ECO.REL.PKG (artifact).

## Decomposition hints

RTRIP (round-trip suite), RO (read-only presentation and refusal), OPENIN (hand-off and write lock), EDITORS (which apps can edit).

## Open questions

- Settled by D-031 item 4 (now CONSTITUTION §3.1): launching another app is not a command. Only `app.list_editors@1` is in the registry; the hand-off is a UI or OS action owned by ECO.APPS.HANDOFF.OPENIN.
- Cross-domain (CORE.BUS): ECO.APPS.HANDOFF.RO needs a generic pre-dispatch guard hook on the bus that an app can install. Raise with CORE.BUS.PIPE, which is still open.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| ECO.APPS.HANDOFF.EDITORS | Which installed apps can edit an element (leaf) | 003 | none |
| ECO.APPS.HANDOFF.RO | Edit guard and read-only provenance for foreign elements (leaf) | 002 003 | none |
| ECO.APPS.HANDOFF.OPENIN | Open-in hand-off with write-lock release (leaf) | 004 | none |
| ECO.APPS.HANDOFF.RTRIP | Cross-app round-trip suite (leaf) | 001 005 | none |
